//! hearsay-stream: file/live decoder streaming over ONE recognizer session
//! (bead hearsay-four-6tc, L6 streaming + voice activity endpointing).
//!
//! Both modes share `runner::decode_stream::EndpointedSession` plus the VAD:
//! `--input file.wav` feeds file-sized chunks, `--live` replays the decoded
//! input in mic-sized frames through the same `LiveSource` seam `serve
//! --live` drives (hermetic live, no physical mic). Stdout is one JSON
//! report; stderr carries provisional/stable lines as regions commit.
//!
//! Exit codes: 0 streamed (silence yields empty stable text — a mic may
//! legitimately hear nothing); 2 usage/input failure; 1 decode failure.

use std::io::{IsTerminal, Read};
use std::path::PathBuf;

use corpus::audio::Pcm16kAudio;
use decoder::{DecoderConfig, MooreDecoder};
use runner::decode_stream::{EndpointedSession, StreamEvent, run_endpointed};
use runner::live::{LiveSource, PcmFrameStream};
use runner::vad::VadConfig;

const USAGE: &str = "\
hearsay-stream --am <model.json> --input <wav|mp3|-> [options]\n\
\n\
Stream one input through the decoder with VAD endpointing. File and live\n\
modes share one EndpointedSession over one MooreDecoder.\n\
\n\
  --am <path>          acoustic model JSON (e.g. models/si_base/model.json)\n\
  --lm <path>          language model file (optional, LM inside search)\n\
  --input <path|->     audio file, or - for stdin bytes\n\
  --live               live replay: mic-sized frames via the LiveSource seam\n\
  --frame-ms <n>       live frame size in ms (default 100, live mode)\n\
  --chunk-ms <n>       file chunk size in ms (default 200, file mode)\n\
  --max-region-secs <f> VAD max region seconds (default 4.0)\n\
  --help               print this help\n\
";

struct StreamRequest {
    am: PathBuf,
    lm: Option<PathBuf>,
    input: Option<PathBuf>,
    live: bool,
    frame_ms: u64,
    chunk_ms: u64,
    max_region_secs: f64,
}

enum StreamAction {
    Help,
    Run(StreamRequest),
}

struct StreamFailure {
    stderr: String,
    code: i32,
}

impl StreamFailure {
    fn usage(message: String) -> Self {
        Self {
            stderr: format!("hearsay-stream: {message}\n{USAGE}"),
            code: 2,
        }
    }

    fn input(message: String) -> Self {
        Self {
            stderr: format!("hearsay-stream: {message}\n"),
            code: 2,
        }
    }

    fn decode(message: String) -> Self {
        Self {
            stderr: format!("hearsay-stream: {message}\n"),
            code: 1,
        }
    }
}

fn parse_u64(flag: &str, value: &str) -> Result<u64, StreamFailure> {
    value.parse::<u64>().map_err(|_| {
        StreamFailure::usage(format!("{flag} must be a positive integer, got {value:?}"))
    })
}

fn parse_f64(flag: &str, value: &str) -> Result<f64, StreamFailure> {
    value
        .parse::<f64>()
        .map_err(|_| StreamFailure::usage(format!("{flag} must be a number, got {value:?}")))
}

fn parse_args(args: &[String]) -> Result<StreamAction, StreamFailure> {
    let mut am: Option<PathBuf> = None;
    let mut lm: Option<PathBuf> = None;
    let mut input: Option<PathBuf> = None;
    let mut live = false;
    let mut frame_ms = 100u64;
    let mut chunk_ms = 200u64;
    let mut max_region_secs = 4.0f64;
    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--help" => return Ok(StreamAction::Help),
            "--live" => live = true,
            "--am" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| StreamFailure::usage("--am needs a path".to_string()))?;
                am = Some(PathBuf::from(value));
            }
            "--lm" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| StreamFailure::usage("--lm needs a path".to_string()))?;
                lm = Some(PathBuf::from(value));
            }
            "--input" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| StreamFailure::usage("--input needs a path".to_string()))?;
                input = Some(PathBuf::from(value));
            }
            "--frame-ms" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| StreamFailure::usage("--frame-ms needs a value".to_string()))?;
                frame_ms = parse_u64("--frame-ms", value)?;
            }
            "--chunk-ms" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| StreamFailure::usage("--chunk-ms needs a value".to_string()))?;
                chunk_ms = parse_u64("--chunk-ms", value)?;
            }
            "--max-region-secs" => {
                i += 1;
                let value = args.get(i).ok_or_else(|| {
                    StreamFailure::usage("--max-region-secs needs a value".to_string())
                })?;
                max_region_secs = parse_f64("--max-region-secs", value)?;
            }
            other => return Err(StreamFailure::usage(format!("unknown flag {other:?}"))),
        }
        i += 1;
    }
    let am = am.ok_or_else(|| StreamFailure::usage("--am is required".to_string()))?;
    if frame_ms == 0 {
        return Err(StreamFailure::usage("--frame-ms must be >= 1".to_string()));
    }
    if chunk_ms == 0 {
        return Err(StreamFailure::usage("--chunk-ms must be >= 1".to_string()));
    }
    if !max_region_secs.is_finite() || max_region_secs <= 0.0 {
        return Err(StreamFailure::usage(
            "--max-region-secs must be finite and > 0".to_string(),
        ));
    }
    Ok(StreamAction::Run(StreamRequest {
        am,
        lm,
        input,
        live,
        frame_ms,
        chunk_ms,
        max_region_secs,
    }))
}

fn decode_input(
    req: &StreamRequest,
    stdin_bytes: Option<&[u8]>,
) -> Result<Pcm16kAudio, StreamFailure> {
    match (&req.input, stdin_bytes) {
        (Some(path), _) if path.as_os_str() != "-" => corpus::audio::decode_to_pcm16k(path)
            .map_err(|e| StreamFailure::input(format!("input {}: {e}", path.display()))),
        (_, Some(bytes)) => corpus::audio::decode_bytes_to_pcm16k(bytes)
            .map_err(|e| StreamFailure::input(format!("stdin audio: {e}"))),
        _ => Err(StreamFailure::input(
            "no audio: pass --input <file> or pipe WAV/MP3 bytes on stdin".to_string(),
        )),
    }
}

fn load_decoder(req: &StreamRequest) -> Result<MooreDecoder, StreamFailure> {
    MooreDecoder::load(
        &req.am,
        req.lm.as_deref(),
        lexicon::CLOSED_VOCABULARY_CMUDICT,
        DecoderConfig::default(),
    )
    .map_err(|e| StreamFailure::decode(format!("decoder load: {e}")))
}

/// `(stdout_text, stderr_text, exit_code)`. Stdout holds the JSON report on
/// success and stays empty on any failure.
fn run_request(req: &StreamRequest, stdin_bytes: Option<&[u8]>) -> (String, String, i32) {
    let fail = |failure: StreamFailure| (String::new(), failure.stderr, failure.code);
    let pcm = match decode_input(req, stdin_bytes) {
        Ok(pcm) => pcm,
        Err(failure) => return fail(failure),
    };
    if pcm.frames.is_empty() {
        return fail(StreamFailure::input(
            "input holds zero frames (no live input)".to_string(),
        ));
    }
    let decoder = match load_decoder(req) {
        Ok(decoder) => decoder,
        Err(failure) => return fail(failure),
    };
    let vocab = decoder.vocab_size();
    let vad = VadConfig {
        max_region_secs: req.max_region_secs,
        ..VadConfig::default()
    };
    if req.live {
        run_live(req, decoder, &pcm, &vad, vocab)
    } else {
        run_file(req, decoder, &pcm, &vad, vocab)
    }
}

/// Inputs to the stdout JSON report (one struct instead of a long
/// argument list).
struct ReportInputs<'a> {
    stable_text: &'a str,
    stable_words: usize,
    segments: &'a [runner::vad::SpeechSegment],
    events: &'a [StreamEvent],
    audio_secs: f64,
    processing_secs: f64,
    session_rtf: f64,
    decoder_rtf: Option<&'a runner::rtf::RtfReport>,
    max_latency_ms: f64,
    p95_latency_ms: f64,
    model: &'a str,
    vocab: usize,
    mode: &'a str,
}

fn report_json(inputs: &ReportInputs<'_>) -> String {
    let segments_json: Vec<serde_json::Value> = inputs
        .segments
        .iter()
        .map(|s| serde_json::json!({"start_ms": s.start_ms, "end_ms": s.end_ms}))
        .collect();
    let provisional_count = inputs
        .events
        .iter()
        .filter(|e| matches!(e, StreamEvent::Provisional(_)))
        .count();
    let stable_count = inputs
        .events
        .iter()
        .filter(|e| matches!(e, StreamEvent::Stable { .. }))
        .count();
    let report = serde_json::json!({
        "mode": inputs.mode,
        "model": inputs.model,
        "vocab": inputs.vocab,
        "stable_text": inputs.stable_text,
        "stable_words": inputs.stable_words,
        "segments": segments_json,
        "provisional_events": provisional_count,
        "stable_events": stable_count,
        "audio_secs": inputs.audio_secs,
        "processing_secs": inputs.processing_secs,
        "session_rtf": inputs.session_rtf,
        "decoder_overall_rtf": inputs.decoder_rtf.map(|r| r.overall_rtf),
        "decoder_mean_rtf": inputs.decoder_rtf.map(|r| r.mean_rtf),
        "decoder_p95_rtf": inputs.decoder_rtf.map(|r| r.p95_rtf),
        "max_latency_ms": inputs.max_latency_ms,
        "p95_latency_ms": inputs.p95_latency_ms,
    });
    format!("{report}\n")
}

fn event_stderr(events: &[StreamEvent]) -> String {
    let mut stderr = String::new();
    for event in events {
        match event {
            StreamEvent::Provisional(h) => {
                stderr.push_str(&format!(
                    "stream: provisional ({} words): {} {}\n",
                    h.total_words, h.stable_text, h.provisional_text,
                ));
            }
            StreamEvent::Stable { segment, text } => {
                stderr.push_str(&format!(
                    "stream: stable [{}-{} ms]: {text}\n",
                    segment.start_ms, segment.end_ms,
                ));
            }
        }
    }
    stderr
}

fn run_file(
    req: &StreamRequest,
    decoder: MooreDecoder,
    pcm: &Pcm16kAudio,
    vad: &VadConfig,
    vocab: usize,
) -> (String, String, i32) {
    let chunk_samples = (req.chunk_ms as usize * 16).max(1);
    let outcome = match run_endpointed(
        decoder,
        MooreDecoder::reset_stream,
        &pcm.frames,
        pcm.sample_rate,
        vad,
        chunk_samples,
    ) {
        Ok(outcome) => outcome,
        Err(e) => {
            let failure = StreamFailure::decode(format!("streaming failed: {e}"));
            return (String::new(), failure.stderr, failure.code);
        }
    };
    let session = &outcome.session;
    let decoder_rtf = session.session().rtf_report().ok();
    let model = req.am.display().to_string();
    let stdout = report_json(&ReportInputs {
        stable_text: session.stable_text(),
        stable_words: session.stable_words(),
        segments: session.segments(),
        events: &outcome.events,
        audio_secs: session.total_secs(),
        processing_secs: session.session().processing_secs(),
        session_rtf: session.session_rtf(),
        decoder_rtf: decoder_rtf.as_ref(),
        max_latency_ms: session.session().max_latency_ms(),
        p95_latency_ms: session.session().p95_latency_ms(),
        model: &model,
        vocab,
        mode: "file",
    });
    (stdout, event_stderr(&outcome.events), 0)
}

fn run_live(
    req: &StreamRequest,
    decoder: MooreDecoder,
    pcm: &Pcm16kAudio,
    vad: &VadConfig,
    vocab: usize,
) -> (String, String, i32) {
    let fail = |failure: StreamFailure| (String::new(), failure.stderr, failure.code);
    let mut source = match PcmFrameStream::new(pcm, req.frame_ms) {
        Ok(source) => source,
        Err(e) => return fail(StreamFailure::input(format!("live frame setup: {e}"))),
    };
    let mut session = match EndpointedSession::new(
        decoder,
        MooreDecoder::reset_stream,
        pcm.sample_rate,
        vad.clone(),
    ) {
        Ok(session) => session,
        Err(e) => return fail(StreamFailure::decode(format!("session setup: {e}"))),
    };
    let mut events = Vec::new();
    while let Some(next) = source.next_chunk() {
        let chunk = match next {
            Ok(chunk) => chunk,
            Err(e) => return fail(StreamFailure::input(format!("live source: {e}"))),
        };
        match session.feed(&chunk) {
            Ok(fed) => events.extend(fed),
            Err(e) => return fail(StreamFailure::decode(format!("streaming failed: {e}"))),
        }
    }
    match session.finish() {
        Ok(done) => events.extend(done),
        Err(e) => return fail(StreamFailure::decode(format!("streaming failed: {e}"))),
    }
    let decoder_rtf = session.session().rtf_report().ok();
    let model = req.am.display().to_string();
    let stdout = report_json(&ReportInputs {
        stable_text: session.stable_text(),
        stable_words: session.stable_words(),
        segments: session.segments(),
        events: &events,
        audio_secs: session.total_secs(),
        processing_secs: session.session().processing_secs(),
        session_rtf: session.session_rtf(),
        decoder_rtf: decoder_rtf.as_ref(),
        max_latency_ms: session.session().max_latency_ms(),
        p95_latency_ms: session.session().p95_latency_ms(),
        model: &model,
        vocab,
        mode: "live",
    });
    (stdout, event_stderr(&events), 0)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let req = match parse_args(&args) {
        Ok(StreamAction::Help) => {
            println!("{USAGE}");
            return;
        }
        Ok(StreamAction::Run(req)) => req,
        Err(failure) => {
            eprint!("{}", failure.stderr);
            std::process::exit(failure.code);
        }
    };
    let wants_stdin = match &req.input {
        None => true,
        Some(path) => path.as_os_str() == "-",
    };
    let stdin_bytes: Option<Vec<u8>> = if !wants_stdin || std::io::stdin().is_terminal() {
        None
    } else {
        let mut buf = Vec::new();
        match std::io::stdin().read_to_end(&mut buf) {
            Ok(_) => Some(buf),
            Err(e) => {
                eprintln!("hearsay-stream: stdin read failed: {e}");
                std::process::exit(1);
            }
        }
    };
    let (stdout_text, stderr_text, code) = run_request(&req, stdin_bytes.as_deref());
    if !stdout_text.is_empty() {
        print!("{stdout_text}");
    }
    if !stderr_text.is_empty() {
        eprint!("{stderr_text}");
    }
    std::process::exit(code);
}

#[cfg(test)]
mod stream_tests {
    use super::*;
    use std::io::Cursor;

    fn am_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../models/si_base/model.json")
    }

    /// 16 kHz mono WAV bytes: `speech_secs` of 440 Hz sine then `sil_secs`
    /// of digital silence.
    fn sine_wav_bytes(speech_secs: f64, sil_secs: f64) -> Vec<u8> {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut writer = hound::WavWriter::new(&mut cursor, spec).expect("wav writer");
            let speech_n = (16_000.0 * speech_secs) as usize;
            for n in 0..speech_n {
                let sample = (8000.0
                    * (2.0 * std::f64::consts::PI * 440.0 * n as f64 / 16_000.0).sin())
                .round() as i16;
                writer.write_sample(sample).expect("write sine");
            }
            let sil_n = (16_000.0 * sil_secs) as usize;
            for _ in 0..sil_n {
                writer.write_sample(0i16).expect("write silence");
            }
            writer.finalize().expect("finalize wav");
        }
        cursor.into_inner()
    }

    fn file_request() -> StreamRequest {
        StreamRequest {
            am: am_path(),
            lm: None,
            input: None,
            live: false,
            frame_ms: 100,
            chunk_ms: 200,
            max_region_secs: 4.0,
        }
    }

    fn live_request() -> StreamRequest {
        StreamRequest {
            live: true,
            ..file_request()
        }
    }

    /// File and live modes share one session type over one decoder: same
    /// stdin bytes through both chunkings commit identical stable text.
    #[test]
    fn file_and_live_modes_commit_identical_stable() {
        let bytes = sine_wav_bytes(1.5, 0.5);
        let (file_out, _, file_code) = run_request(&file_request(), Some(&bytes));
        assert_eq!(file_code, 0);
        let (live_out, _, live_code) = run_request(&live_request(), Some(&bytes));
        assert_eq!(live_code, 0);
        let file_json: serde_json::Value =
            serde_json::from_str(&file_out).expect("file stdout is JSON");
        let live_json: serde_json::Value =
            serde_json::from_str(&live_out).expect("live stdout is JSON");
        assert_eq!(file_json["mode"], serde_json::json!("file"));
        assert_eq!(live_json["mode"], serde_json::json!("live"));
        assert_eq!(
            file_json["stable_text"], live_json["stable_text"],
            "one decoder: file and live agree"
        );
        assert_eq!(file_json["segments"], live_json["segments"]);
    }

    #[test]
    fn report_carries_measurements() {
        let bytes = sine_wav_bytes(0.5, 0.2);
        let (stdout, _, code) = run_request(&file_request(), Some(&bytes));
        assert_eq!(code, 0);
        let json: serde_json::Value = serde_json::from_str(&stdout).expect("JSON report");
        for key in [
            "mode",
            "model",
            "vocab",
            "stable_text",
            "stable_words",
            "segments",
            "audio_secs",
            "processing_secs",
            "session_rtf",
            "max_latency_ms",
            "p95_latency_ms",
        ] {
            assert!(json.get(key).is_some(), "report carries {key}");
        }
        assert_eq!(json["vocab"], serde_json::json!(30));
    }

    #[test]
    fn usage_and_input_failures_exit_2_without_stdout() {
        // Unknown flag.
        let action = parse_args(&["--bogus".to_string()]);
        assert!(action.is_err());
        // Missing --am.
        let action = parse_args(&["--input".to_string(), "x.wav".to_string()]);
        match action {
            Ok(_) => panic!("missing --am must fail"),
            Err(failure) => assert_eq!(failure.code, 2),
        }
        // Zero frame size.
        let action = parse_args(&[
            "--am".to_string(),
            "m.json".to_string(),
            "--frame-ms".to_string(),
            "0".to_string(),
        ]);
        assert!(action.is_err());
        // No audio anywhere.
        let (stdout, _, code) = run_request(&file_request(), None);
        assert_eq!(code, 2);
        assert!(stdout.is_empty());
        // Garbage stdin bytes.
        let (stdout, _, code) = run_request(&file_request(), Some(b"not audio"));
        assert_eq!(code, 2);
        assert!(stdout.is_empty());
    }

    #[test]
    fn missing_model_exits_1_without_stdout() {
        let req = StreamRequest {
            am: PathBuf::from("/nonexistent/model.json"),
            ..file_request()
        };
        let bytes = sine_wav_bytes(0.5, 0.0);
        let (stdout, _, code) = run_request(&req, Some(&bytes));
        assert_eq!(code, 1);
        assert!(stdout.is_empty());
    }
}
