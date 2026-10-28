# Recording Protocol — owner audio + LOCKBOX sealing (bead hearsay-four-7ms, D1)

Binding for every owner recording session from D1 onward. Partitions
are defined in [D1-PARTITIONS.md](/home/michael/repos/hearsay-four-lane-d1/docs/results/D1-PARTITIONS.md);
split manifests live in `data/partitions/`.

## 1. Technical standard (no exceptions)

| Item | Standard |
|---|---|
| Format | WAV, PCM 16-bit, **16 kHz, mono** (`record-session` output verified before commit) |
| Mic | Same mic class for every session in a partition family; mic model recorded in the session TSV header |
| Room | Quiet room, door closed, no HVAC/fan audible; same room where practical |
| Distance/level | Fixed mic distance (record cm in header); peaks below -3 dBFS, no clipping |
| Takes | One utterance per file; lead/trail silence kept (SIL/SP templates come from it) |

A session that fails the rate/mono check is re-recorded, never resampled
into a partition silently. (Existing 48 kHz `sessions/run1` stays
NOT-VERIFIED/excluded until its WAVs are present; see D1 exclusions.)

## 2. Session naming

```
owner-<YYYYMMDD>-<PART>-<LIST>-t<NN>.wav      (one utterance per file)
session dir: sessions/owner-<YYYYMMDD>-<PART>-<LIST>/
  session.tsv   (header: mic, room, distance, rate; rows: file, prompt, take)
  manifest.sha256
```

`<PART>` ∈ `TRAIN|DEV|PHASE-TEST|LOCKBOX` is assigned **before recording**
and written in the dir name. `<LIST>` ∈ `harvard`, `phrases30`, `p4`,
`enroll`. A session belongs to exactly one partition forever; a 5th take
for A1 is a **new** PHASE-TEST session, never an addition to DEV.

## 3. Prompt lists

### 3.1 Harvard sentences (phonetically balanced, public domain)

Record list by list; List 1 (of 72×10) is the enrollment starter:

1. The birch canoe slid on the smooth planks.
2. Glue the sheet to the dark blue background.
3. It's easy to tell the depth of a well.
4. These days a chicken leg is a rare dish.
5. Rice is often served in round bowls.
6. The juice of lemons makes fine punch.
7. The box was thrown beside the parked truck.
8. The hogs were fed chopped corn and garbage.
9. Four hours of steady work faced us.
10. A large size in stockings is hard to sell.

### 3.2 30-word phrases (closed vocabulary, A2)

Vocabulary: the 30 words of `CLOSED_VOCABULARY` in `src/fse/src/lib.rs`
(impeachment, methodical, survival, process, intense, reflection,
kinship, resented, congealed, uncorroborated, highboy, hotbed, ripple,
predestine, byzantium, genetic, constitution, interpersonal,
enchantment, perfume, submissions, overboard, knockdown, sleepwalking,
unrealized, panhandling, horribly, salivating, rhubarb, staked).
Prompts are connected 3–7-word phrases over this vocabulary only
(generated lists: `sessions/script-wave2.txt`, `script-wave3.txt`
pattern). Example prompts: "survival process intense reflection",
"knockdown ripple overboard staked", "horribly congealed rhubarb".
DEV phrases may be TTS (D0 sets); PHASE-TEST phrases are owner human
speech in their own session (≥100 phrases, per master plan A2).

### 3.3 P4 target sentences (targeted-confusion benchmark)

Per confusion pair (starter pair: **fly → family**), record in natural
contexts, plus competitor-correct contexts:

- ≥20 DEV sentences containing the target ("a fly landed on the rim",
  "the fly was caught in the web", …) — corrections may come from DEV only.
- ≥20 PHASE-TEST sentences containing the target, in a separate later
  session, never used for corrections.
- ≥20 competitor-correct sentences ("my family lives in Ohio",
  "the family gathered at noon", …) split across the same two sessions
  by session, never by sentence within a session.

New pairs are added the same way: pair name, target, competitor, and
the two session ids are registered in the D1 results doc before scoring.

## 4. LOCKBOX sealing and opening

1. Record LOCKBOX sessions/cases last, chronologically after PHASE-TEST.
2. Seal: run `scripts/build_d1_partitions.py`; the LOCKBOX manifest
   (`data/partitions/lockbox.json`) and its combined sha256 in
   `data/partitions/manifest-index.json` are committed. That commit is
   the seal. Files are **not read by any pipeline** after sealing.
3. Opening: at most once per major milestone (M2, M4, M5). An opening
   appends `{milestone, date, manifest_sha256}` to `openings[]` in the
   seal block, is logged in the D1 bead notes, and the case then becomes
   REGRESSION (known-result benchmark), never "test" again.
4. `scripts/test_d1_partitions.py` fails if any ordinary evaluation
   config references a LOCKBOX file outside the seal machinery.

Current LOCKBOX contents and seal values: [D1-PARTITIONS.md](/home/michael/repos/hearsay-four-lane-d1/docs/results/D1-PARTITIONS.md).
