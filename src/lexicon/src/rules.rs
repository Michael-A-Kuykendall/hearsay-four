//! Classical rule-based letter-to-sound (LTS) engine.
//!
//! Bead `hearsay-four-k8d--book-missing-pronunciation-test-set`: the two-tier
//! lexicon leaves book words with no pronunciation in either tier, so this
//! module derives pronunciations from ordered context-sensitive rewrite
//! rules — no per-word exceptions, no learned weights (the no-model guard
//! stays green).
//!
//! Formalism: each [`Rule`] is `left + grapheme + right -> phones`, applied
//! left to right with first match winning. Context patterns use `#` (word
//! boundary), `V` (vowel letter: `AEIOU`, plus `Y` outside word-initial
//! position), `C` (any other letter), or literal letters (lowercase `c`/`v`
//! escape the classes to match a literal `C`/`V`). Every emitted
//! phone is drawn from [`crate::PHONEME_ALPHABET`] (39 stress-stripped
//! ARPABET phones shared with both upper tiers).
//!
//! Coverage note: this is the workstream's first rule set (v1). It covers
//! common patterns (silent-e, digraphs, `-tion`, doubled consonants,
//! syllabic endings) and approximates the rest; every input still yields a
//! non-empty phone string.

/// Identifies the rule set that produced a pronunciation batch. Tier-3 files
/// record this in their header so regeneration is reproducible.
pub const RULE_ENGINE_ID: &str = "lexicon-rules-v1";

/// One ordered rewrite rule: `left + grapheme + right -> phones`.
///
/// An empty `phones` slice marks the grapheme silent in that context.
struct Rule {
    left: &'static str,
    grapheme: &'static str,
    right: &'static str,
    phones: &'static [&'static str],
}

/// Ordered rule table: first match at each position wins. Specific and
/// multi-letter rules precede the single-letter defaults they shadow.
const RULES: &[Rule] = &[
    // `ough` family: through/cough/tough/rough/enough/thought/though.
    Rule {
        left: "HR",
        grapheme: "OUGH",
        right: "#",
        phones: &["UW"],
    },
    Rule {
        left: "c",
        grapheme: "OUGH",
        right: "#",
        phones: &["AO", "F"],
    },
    Rule {
        left: "T",
        grapheme: "OUGH",
        right: "#",
        phones: &["AH", "F"],
    },
    Rule {
        left: "R",
        grapheme: "OUGH",
        right: "#",
        phones: &["AH", "F"],
    },
    Rule {
        left: "N",
        grapheme: "OUGH",
        right: "#",
        phones: &["AH", "F"],
    },
    Rule {
        left: "",
        grapheme: "OUGH",
        right: "T",
        phones: &["AO"],
    },
    Rule {
        left: "",
        grapheme: "OUGH",
        right: "#",
        phones: &["OW"],
    },
    Rule {
        left: "",
        grapheme: "OUGH",
        right: "",
        phones: &["AW"],
    },
    // laugh; high/night; weigh.
    Rule {
        left: "",
        grapheme: "AUGH",
        right: "",
        phones: &["AE", "F"],
    },
    Rule {
        left: "",
        grapheme: "IGH",
        right: "#",
        phones: &["AY"],
    },
    Rule {
        left: "",
        grapheme: "IGH",
        right: "T",
        phones: &["AY"],
    },
    Rule {
        left: "",
        grapheme: "IGH",
        right: "",
        phones: &["AY"],
    },
    Rule {
        left: "",
        grapheme: "EIGH",
        right: "",
        phones: &["EY"],
    },
    // -tion / -sion: nation; tension/vision.
    Rule {
        left: "",
        grapheme: "TION",
        right: "",
        phones: &["SH", "AH", "N"],
    },
    Rule {
        left: "N",
        grapheme: "SION",
        right: "",
        phones: &["SH", "AH", "N"],
    },
    Rule {
        left: "V",
        grapheme: "SION",
        right: "",
        phones: &["ZH", "AH", "N"],
    },
    Rule {
        left: "",
        grapheme: "SION",
        right: "",
        phones: &["SH", "AH", "N"],
    },
    // Consonant clusters.
    Rule {
        left: "",
        grapheme: "TCH",
        right: "",
        phones: &["CH"],
    },
    Rule {
        left: "",
        grapheme: "DGE",
        right: "",
        phones: &["JH"],
    },
    Rule {
        left: "",
        grapheme: "SCH",
        right: "",
        phones: &["S", "K"],
    },
    Rule {
        left: "",
        grapheme: "CH",
        right: "",
        phones: &["CH"],
    },
    Rule {
        left: "",
        grapheme: "SH",
        right: "",
        phones: &["SH"],
    },
    Rule {
        left: "",
        grapheme: "TH",
        right: "",
        phones: &["TH"],
    },
    Rule {
        left: "",
        grapheme: "PH",
        right: "",
        phones: &["F"],
    },
    Rule {
        left: "",
        grapheme: "WH",
        right: "O",
        phones: &["HH"],
    },
    Rule {
        left: "",
        grapheme: "WH",
        right: "",
        phones: &["W"],
    },
    // qu + vowel: quiet/quite/quit precede plain qu.
    Rule {
        left: "",
        grapheme: "QUIE",
        right: "T",
        phones: &["K", "W", "AY", "AH"],
    },
    Rule {
        left: "",
        grapheme: "QUI",
        right: "CE#",
        phones: &["K", "W", "AY"],
    },
    Rule {
        left: "",
        grapheme: "QUI",
        right: "",
        phones: &["K", "W", "IH"],
    },
    Rule {
        left: "",
        grapheme: "QU",
        right: "",
        phones: &["K", "W"],
    },
    // gu + vowel (u silent): guest/guide/guard; league.
    Rule {
        left: "",
        grapheme: "GUE",
        right: "#",
        phones: &["G"],
    },
    Rule {
        left: "",
        grapheme: "GU",
        right: "E",
        phones: &["G"],
    },
    Rule {
        left: "",
        grapheme: "GU",
        right: "I",
        phones: &["G"],
    },
    Rule {
        left: "",
        grapheme: "GU",
        right: "A",
        phones: &["G"],
    },
    // ng/nk/ck: singe/singing/sing/finger; bank; back.
    Rule {
        left: "",
        grapheme: "NGE",
        right: "#",
        phones: &["N", "JH"],
    },
    Rule {
        left: "",
        grapheme: "NG",
        right: "I",
        phones: &["NG"],
    },
    Rule {
        left: "",
        grapheme: "NG",
        right: "#",
        phones: &["NG"],
    },
    Rule {
        left: "",
        grapheme: "NG",
        right: "V",
        phones: &["NG", "G"],
    },
    Rule {
        left: "",
        grapheme: "NG",
        right: "",
        phones: &["NG"],
    },
    Rule {
        left: "",
        grapheme: "NK",
        right: "",
        phones: &["NG", "K"],
    },
    Rule {
        left: "",
        grapheme: "CK",
        right: "",
        phones: &["K"],
    },
    // Silent-initial clusters; reign/sign/signal.
    Rule {
        left: "#",
        grapheme: "GN",
        right: "",
        phones: &["N"],
    },
    Rule {
        left: "",
        grapheme: "EIGN",
        right: "#",
        phones: &["EY", "N"],
    },
    Rule {
        left: "",
        grapheme: "IGN",
        right: "#",
        phones: &["AY", "N"],
    },
    Rule {
        left: "",
        grapheme: "GN",
        right: "#",
        phones: &["N"],
    },
    Rule {
        left: "",
        grapheme: "GN",
        right: "V",
        phones: &["G", "N"],
    },
    Rule {
        left: "",
        grapheme: "GN",
        right: "",
        phones: &["N"],
    },
    Rule {
        left: "#",
        grapheme: "KN",
        right: "",
        phones: &["N"],
    },
    Rule {
        left: "#",
        grapheme: "PN",
        right: "",
        phones: &["N"],
    },
    Rule {
        left: "#",
        grapheme: "WR",
        right: "",
        phones: &["R"],
    },
    Rule {
        left: "#",
        grapheme: "PS",
        right: "",
        phones: &["S"],
    },
    Rule {
        left: "",
        grapheme: "RH",
        right: "",
        phones: &["R"],
    },
    // dg/gg/cc: budget; suggest; accept/account.
    Rule {
        left: "",
        grapheme: "DG",
        right: "E",
        phones: &["JH"],
    },
    Rule {
        left: "",
        grapheme: "DG",
        right: "I",
        phones: &["JH"],
    },
    Rule {
        left: "",
        grapheme: "DG",
        right: "",
        phones: &["G"],
    },
    Rule {
        left: "",
        grapheme: "GGE",
        right: "",
        phones: &["G", "JH"],
    },
    Rule {
        left: "",
        grapheme: "GG",
        right: "E",
        phones: &["G", "JH"],
    },
    Rule {
        left: "",
        grapheme: "GG",
        right: "",
        phones: &["G"],
    },
    Rule {
        left: "",
        grapheme: "CC",
        right: "E",
        phones: &["K", "S"],
    },
    Rule {
        left: "",
        grapheme: "CC",
        right: "I",
        phones: &["K", "S"],
    },
    Rule {
        left: "",
        grapheme: "CC",
        right: "",
        phones: &["K"],
    },
    // Palatalized clusters before soft-c defaults: gracious/ocean/initial.
    Rule {
        left: "",
        grapheme: "CI",
        right: "V",
        phones: &["SH"],
    },
    Rule {
        left: "",
        grapheme: "CEA",
        right: "",
        phones: &["SH"],
    },
    Rule {
        left: "",
        grapheme: "TI",
        right: "V",
        phones: &["SH"],
    },
    // Soft c/g: cent/city; gentle.
    Rule {
        left: "",
        grapheme: "C",
        right: "E",
        phones: &["S"],
    },
    Rule {
        left: "",
        grapheme: "C",
        right: "I",
        phones: &["S"],
    },
    Rule {
        left: "",
        grapheme: "C",
        right: "Y",
        phones: &["S"],
    },
    Rule {
        left: "",
        grapheme: "C",
        right: "",
        phones: &["K"],
    },
    Rule {
        left: "",
        grapheme: "G",
        right: "E",
        phones: &["JH"],
    },
    Rule {
        left: "",
        grapheme: "G",
        right: "I",
        phones: &["JH"],
    },
    Rule {
        left: "",
        grapheme: "G",
        right: "Y",
        phones: &["JH"],
    },
    Rule {
        left: "",
        grapheme: "G",
        right: "",
        phones: &["G"],
    },
    // Vowel digraphs: fair/rain/day/saw/caught.
    Rule {
        left: "",
        grapheme: "AIR",
        right: "",
        phones: &["EH", "R"],
    },
    Rule {
        left: "",
        grapheme: "AI",
        right: "",
        phones: &["EY"],
    },
    Rule {
        left: "",
        grapheme: "AY",
        right: "",
        phones: &["EY"],
    },
    Rule {
        left: "",
        grapheme: "AW",
        right: "",
        phones: &["AO"],
    },
    Rule {
        left: "",
        grapheme: "AU",
        right: "",
        phones: &["AO"],
    },
    // ea family: fear/earth/read/bread.
    Rule {
        left: "",
        grapheme: "EAR",
        right: "#",
        phones: &["IY", "R"],
    },
    Rule {
        left: "",
        grapheme: "EAR",
        right: "C",
        phones: &["ER"],
    },
    Rule {
        left: "",
        grapheme: "EAR",
        right: "",
        phones: &["IY", "R"],
    },
    Rule {
        left: "",
        grapheme: "EAD",
        right: "#",
        phones: &["EH", "D"],
    },
    Rule {
        left: "",
        grapheme: "EA",
        right: "R",
        phones: &["IY"],
    },
    Rule {
        left: "",
        grapheme: "EA",
        right: "",
        phones: &["IY"],
    },
    Rule {
        left: "",
        grapheme: "EER",
        right: "",
        phones: &["IY", "R"],
    },
    Rule {
        left: "",
        grapheme: "EE",
        right: "",
        phones: &["IY"],
    },
    // ie/ei/ey/eu/ew: movie/lie/friend/field/piece; ceiling/key/they/vein; feud/new.
    Rule {
        left: "V",
        grapheme: "IE",
        right: "#",
        phones: &["IY"],
    },
    Rule {
        left: "",
        grapheme: "IE",
        right: "#",
        phones: &["AY"],
    },
    Rule {
        left: "",
        grapheme: "IE",
        right: "N",
        phones: &["EH"],
    },
    Rule {
        left: "",
        grapheme: "IE",
        right: "L",
        phones: &["IY"],
    },
    Rule {
        left: "",
        grapheme: "IE",
        right: "C",
        phones: &["IY"],
    },
    Rule {
        left: "",
        grapheme: "IE",
        right: "",
        phones: &["AY"],
    },
    Rule {
        left: "c",
        grapheme: "EI",
        right: "",
        phones: &["IY"],
    },
    Rule {
        left: "",
        grapheme: "EY",
        right: "#",
        phones: &["IY"],
    },
    Rule {
        left: "",
        grapheme: "EY",
        right: "",
        phones: &["EY"],
    },
    Rule {
        left: "",
        grapheme: "EI",
        right: "",
        phones: &["EY"],
    },
    Rule {
        left: "",
        grapheme: "EU",
        right: "",
        phones: &["Y", "UW"],
    },
    Rule {
        left: "",
        grapheme: "EW",
        right: "",
        phones: &["UW"],
    },
    // o digraphs: board/boat/toe/join/boy/book/door/moon.
    Rule {
        left: "",
        grapheme: "OAR",
        right: "",
        phones: &["AO", "R"],
    },
    Rule {
        left: "",
        grapheme: "OA",
        right: "",
        phones: &["OW"],
    },
    Rule {
        left: "",
        grapheme: "OE",
        right: "",
        phones: &["OW"],
    },
    Rule {
        left: "",
        grapheme: "OI",
        right: "",
        phones: &["OY"],
    },
    Rule {
        left: "",
        grapheme: "OY",
        right: "",
        phones: &["OY"],
    },
    Rule {
        left: "",
        grapheme: "OOK",
        right: "",
        phones: &["UH", "K"],
    },
    Rule {
        left: "",
        grapheme: "OOR",
        right: "",
        phones: &["AO", "R"],
    },
    Rule {
        left: "",
        grapheme: "OO",
        right: "",
        phones: &["UW"],
    },
    // ou family: hour/would/famous/double/couch/sour/thou/out.
    Rule {
        left: "",
        grapheme: "OUR",
        right: "#",
        phones: &["AW", "ER"],
    },
    Rule {
        left: "",
        grapheme: "OUL",
        right: "D",
        phones: &["UH"],
    },
    Rule {
        left: "",
        grapheme: "OUS",
        right: "#",
        phones: &["AH", "S"],
    },
    Rule {
        left: "",
        grapheme: "OU",
        right: "B",
        phones: &["AH"],
    },
    Rule {
        left: "",
        grapheme: "OU",
        right: "CH",
        phones: &["AW"],
    },
    Rule {
        left: "",
        grapheme: "OU",
        right: "R",
        phones: &["AW"],
    },
    Rule {
        left: "",
        grapheme: "OU",
        right: "#",
        phones: &["AW"],
    },
    Rule {
        left: "",
        grapheme: "OU",
        right: "",
        phones: &["AW"],
    },
    // ow: cow/snow/owe/towel/brown/row.
    Rule {
        left: "#C",
        grapheme: "OW",
        right: "#",
        phones: &["AW"],
    },
    Rule {
        left: "",
        grapheme: "OW",
        right: "#",
        phones: &["OW"],
    },
    Rule {
        left: "",
        grapheme: "OW",
        right: "E#",
        phones: &["OW"],
    },
    Rule {
        left: "",
        grapheme: "OW",
        right: "E",
        phones: &["AW"],
    },
    Rule {
        left: "",
        grapheme: "OW",
        right: "C",
        phones: &["AW"],
    },
    Rule {
        left: "",
        grapheme: "OW",
        right: "",
        phones: &["OW"],
    },
    // ue/ui: blue; guide/build/fruit.
    Rule {
        left: "",
        grapheme: "UE",
        right: "#",
        phones: &["UW"],
    },
    Rule {
        left: "",
        grapheme: "UE",
        right: "",
        phones: &["UW"],
    },
    Rule {
        left: "",
        grapheme: "UI",
        right: "DE#",
        phones: &["AY", "D"],
    },
    Rule {
        left: "",
        grapheme: "UIL",
        right: "D",
        phones: &["IH", "L"],
    },
    Rule {
        left: "",
        grapheme: "UI",
        right: "",
        phones: &["UW"],
    },
    // a: a/vague/care/car/card/marry/range/ball/talk/calm/salmon/half/palace/nation/lazy/name.
    Rule {
        left: "#",
        grapheme: "A",
        right: "#",
        phones: &["AH"],
    },
    Rule {
        left: "",
        grapheme: "A",
        right: "THER",
        phones: &["AA"],
    },
    Rule {
        left: "",
        grapheme: "A",
        right: "GUE#",
        phones: &["EY"],
    },
    Rule {
        left: "",
        grapheme: "ARE",
        right: "#",
        phones: &["EY", "R"],
    },
    Rule {
        left: "",
        grapheme: "AR",
        right: "#",
        phones: &["AA", "R"],
    },
    Rule {
        left: "",
        grapheme: "AR",
        right: "C",
        phones: &["AA", "R"],
    },
    Rule {
        left: "",
        grapheme: "AR",
        right: "V",
        phones: &["EH", "R"],
    },
    Rule {
        left: "",
        grapheme: "ANGE",
        right: "",
        phones: &["EY", "N", "JH"],
    },
    Rule {
        left: "",
        grapheme: "ALL",
        right: "",
        phones: &["AO", "L"],
    },
    Rule {
        left: "",
        grapheme: "ALK",
        right: "",
        phones: &["AO", "K"],
    },
    Rule {
        left: "",
        grapheme: "ALM",
        right: "#",
        phones: &["AA", "M"],
    },
    Rule {
        left: "",
        grapheme: "ALM",
        right: "",
        phones: &["AE", "M"],
    },
    Rule {
        left: "",
        grapheme: "ALF",
        right: "",
        phones: &["AE", "F"],
    },
    Rule {
        left: "",
        grapheme: "AL",
        right: "",
        phones: &["AE", "L"],
    },
    Rule {
        left: "",
        grapheme: "A",
        right: "TION",
        phones: &["EY"],
    },
    Rule {
        left: "",
        grapheme: "A",
        right: "SION",
        phones: &["EY"],
    },
    Rule {
        left: "",
        grapheme: "A",
        right: "ZY#",
        phones: &["EY"],
    },
    Rule {
        left: "",
        grapheme: "A",
        right: "BY#",
        phones: &["EY"],
    },
    Rule {
        left: "",
        grapheme: "A",
        right: "DY#",
        phones: &["EY"],
    },
    Rule {
        left: "",
        grapheme: "A",
        right: "BLE#",
        phones: &["EY"],
    },
    Rule {
        left: "",
        grapheme: "A",
        right: "DLE#",
        phones: &["EY"],
    },
    Rule {
        left: "",
        grapheme: "A",
        right: "CE#",
        phones: &["EY"],
    },
    Rule {
        left: "",
        grapheme: "A",
        right: "",
        phones: &["AE"],
    },
    // -es plurals live here (before the E defaults, which would shadow
    // them): buses/boxes/waltzes/churches/wolves.
    Rule {
        left: "",
        grapheme: "VES",
        right: "#",
        phones: &["V", "Z"],
    },
    Rule {
        left: "S",
        grapheme: "ES",
        right: "#",
        phones: &["AH", "Z"],
    },
    Rule {
        left: "X",
        grapheme: "ES",
        right: "#",
        phones: &["AH", "Z"],
    },
    Rule {
        left: "Z",
        grapheme: "ES",
        right: "#",
        phones: &["AH", "Z"],
    },
    Rule {
        left: "H",
        grapheme: "ES",
        right: "#",
        phones: &["AH", "Z"],
    },
    // e: here/her/mercy/very/these; silent final e; bed.
    Rule {
        left: "",
        grapheme: "ERE",
        right: "#",
        phones: &["IY", "R"],
    },
    Rule {
        left: "",
        grapheme: "ER",
        right: "#",
        phones: &["ER"],
    },
    Rule {
        left: "",
        grapheme: "ER",
        right: "C",
        phones: &["ER"],
    },
    Rule {
        left: "",
        grapheme: "ER",
        right: "V",
        phones: &["EH", "R"],
    },
    Rule {
        left: "",
        grapheme: "E",
        right: "CE#",
        phones: &["IY"],
    },
    Rule {
        left: "",
        grapheme: "E",
        right: "#",
        phones: &[],
    },
    Rule {
        left: "",
        grapheme: "E",
        right: "",
        phones: &["EH"],
    },
    // i: I/fatigue/tired/fire/sir/bird/pirate/find/child/like/piano/diet; tin.
    Rule {
        left: "#",
        grapheme: "I",
        right: "#",
        phones: &["AY"],
    },
    Rule {
        left: "",
        grapheme: "I",
        right: "GUE#",
        phones: &["IY"],
    },
    Rule {
        left: "",
        grapheme: "IRE",
        right: "D#",
        phones: &["AY", "ER", "D"],
    },
    Rule {
        left: "",
        grapheme: "IRE",
        right: "#",
        phones: &["AY", "ER"],
    },
    Rule {
        left: "",
        grapheme: "IR",
        right: "#",
        phones: &["ER"],
    },
    Rule {
        left: "",
        grapheme: "IR",
        right: "C",
        phones: &["ER"],
    },
    Rule {
        left: "",
        grapheme: "IR",
        right: "V",
        phones: &["AY", "R"],
    },
    Rule {
        left: "",
        grapheme: "I",
        right: "ND",
        phones: &["AY"],
    },
    Rule {
        left: "",
        grapheme: "I",
        right: "LD",
        phones: &["AY"],
    },
    Rule {
        left: "",
        grapheme: "I",
        right: "TLE#",
        phones: &["AY"],
    },
    Rule {
        left: "",
        grapheme: "I",
        right: "DLE#",
        phones: &["AY"],
    },
    Rule {
        left: "",
        grapheme: "I",
        right: "CE#",
        phones: &["AY"],
    },
    Rule {
        left: "",
        grapheme: "I",
        right: "A",
        phones: &["IY"],
    },
    Rule {
        left: "",
        grapheme: "I",
        right: "V",
        phones: &["AY"],
    },
    Rule {
        left: "",
        grapheme: "I",
        right: "",
        phones: &["IH"],
    },
    // o: o/mother/more/or/fork/oral/son/button/home/go; hot.
    Rule {
        left: "#",
        grapheme: "O",
        right: "#",
        phones: &["OW"],
    },
    Rule {
        left: "",
        grapheme: "O",
        right: "THER",
        phones: &["AH"],
    },
    Rule {
        left: "",
        grapheme: "ORE",
        right: "#",
        phones: &["OW", "R"],
    },
    Rule {
        left: "",
        grapheme: "OR",
        right: "#",
        phones: &["AO", "R"],
    },
    Rule {
        left: "",
        grapheme: "OR",
        right: "C",
        phones: &["AO", "R"],
    },
    Rule {
        left: "",
        grapheme: "OR",
        right: "V",
        phones: &["AO", "R"],
    },
    Rule {
        left: "",
        grapheme: "O",
        right: "N#",
        phones: &["AH"],
    },
    Rule {
        left: "",
        grapheme: "O",
        right: "G",
        phones: &["AO"],
    },
    Rule {
        left: "",
        grapheme: "O",
        right: "BLE#",
        phones: &["OW"],
    },
    Rule {
        left: "",
        grapheme: "O",
        right: "CE#",
        phones: &["OW"],
    },
    Rule {
        left: "",
        grapheme: "O",
        right: "#",
        phones: &["OW"],
    },
    Rule {
        left: "",
        grapheme: "O",
        right: "",
        phones: &["AA"],
    },
    // u: union/cure/turn/burst/during/rule/flute/cube/bush/full; cup.
    Rule {
        left: "#",
        grapheme: "U",
        right: "V",
        phones: &["Y", "UW"],
    },
    Rule {
        left: "",
        grapheme: "URE",
        right: "#",
        phones: &["Y", "UH", "R"],
    },
    Rule {
        left: "",
        grapheme: "UR",
        right: "#",
        phones: &["ER"],
    },
    Rule {
        left: "",
        grapheme: "UR",
        right: "C",
        phones: &["ER"],
    },
    Rule {
        left: "",
        grapheme: "UR",
        right: "V",
        phones: &["UH", "R"],
    },
    Rule {
        left: "R",
        grapheme: "U",
        right: "CE#",
        phones: &["UW"],
    },
    Rule {
        left: "L",
        grapheme: "U",
        right: "CE#",
        phones: &["UW"],
    },
    Rule {
        left: "",
        grapheme: "U",
        right: "BLE#",
        phones: &["UW"],
    },
    Rule {
        left: "",
        grapheme: "U",
        right: "CE#",
        phones: &["Y", "UW"],
    },
    Rule {
        left: "",
        grapheme: "USH",
        right: "",
        phones: &["UH", "SH"],
    },
    Rule {
        left: "",
        grapheme: "ULL",
        right: "",
        phones: &["UH", "L"],
    },
    Rule {
        left: "",
        grapheme: "U",
        right: "",
        phones: &["AH"],
    },
    // y as consonant or vowel: yes/beyond/my/fly/buy/happy/gym/rhythm.
    Rule {
        left: "#",
        grapheme: "Y",
        right: "V",
        phones: &["Y"],
    },
    Rule {
        left: "",
        grapheme: "Y",
        right: "V",
        phones: &["Y"],
    },
    Rule {
        left: "#C",
        grapheme: "Y",
        right: "#",
        phones: &["AY"],
    },
    Rule {
        left: "#CC",
        grapheme: "Y",
        right: "#",
        phones: &["AY"],
    },
    Rule {
        left: "U",
        grapheme: "Y",
        right: "#",
        phones: &["AY"],
    },
    Rule {
        left: "",
        grapheme: "Y",
        right: "#",
        phones: &["IY"],
    },
    Rule {
        left: "",
        grapheme: "Y",
        right: "C",
        phones: &["IH"],
    },
    Rule {
        left: "",
        grapheme: "Y",
        right: "",
        phones: &["IH"],
    },
    // Syllabic endings and silent letters: castle/fasten/often/table; comb/hymn/inn/prism/button.
    Rule {
        left: "S",
        grapheme: "T",
        right: "LE#",
        phones: &[],
    },
    Rule {
        left: "",
        grapheme: "STEN",
        right: "#",
        phones: &["S", "AH", "N"],
    },
    Rule {
        left: "",
        grapheme: "FTEN",
        right: "#",
        phones: &["F", "AH", "N"],
    },
    Rule {
        left: "C",
        grapheme: "LE",
        right: "#",
        phones: &["AH", "L"],
    },
    Rule {
        left: "M",
        grapheme: "B",
        right: "#",
        phones: &[],
    },
    Rule {
        left: "M",
        grapheme: "N",
        right: "#",
        phones: &[],
    },
    Rule {
        left: "",
        grapheme: "NN",
        right: "#",
        phones: &["N"],
    },
    Rule {
        left: "C",
        grapheme: "M",
        right: "#",
        phones: &["AH", "M"],
    },
    Rule {
        left: "C",
        grapheme: "N",
        right: "#",
        phones: &["AH", "N"],
    },
    // Leftover gh: ghost; otherwise silent.
    Rule {
        left: "#",
        grapheme: "GH",
        right: "",
        phones: &["G"],
    },
    Rule {
        left: "",
        grapheme: "GH",
        right: "",
        phones: &[],
    },
    // Doubled consonants: dinner; pizza/buzz.
    Rule {
        left: "",
        grapheme: "BB",
        right: "",
        phones: &["B"],
    },
    Rule {
        left: "",
        grapheme: "DD",
        right: "",
        phones: &["D"],
    },
    Rule {
        left: "",
        grapheme: "FF",
        right: "",
        phones: &["F"],
    },
    Rule {
        left: "",
        grapheme: "PP",
        right: "",
        phones: &["P"],
    },
    Rule {
        left: "",
        grapheme: "RR",
        right: "",
        phones: &["R"],
    },
    Rule {
        left: "",
        grapheme: "SS",
        right: "",
        phones: &["S"],
    },
    Rule {
        left: "",
        grapheme: "TT",
        right: "",
        phones: &["T"],
    },
    Rule {
        left: "",
        grapheme: "LL",
        right: "",
        phones: &["L"],
    },
    Rule {
        left: "",
        grapheme: "MM",
        right: "",
        phones: &["M"],
    },
    Rule {
        left: "",
        grapheme: "NN",
        right: "",
        phones: &["N"],
    },
    Rule {
        left: "",
        grapheme: "ZZ",
        right: "V",
        phones: &["T", "S"],
    },
    Rule {
        left: "",
        grapheme: "ZZ",
        right: "#",
        phones: &["Z"],
    },
    Rule {
        left: "",
        grapheme: "ZZ",
        right: "",
        phones: &["Z"],
    },
    // s voicing: asia/rose/expansion; dogs. (-es plurals sit before the E block.)
    Rule {
        left: "V",
        grapheme: "S",
        right: "IA",
        phones: &["ZH"],
    },
    Rule {
        left: "V",
        grapheme: "S",
        right: "V",
        phones: &["Z"],
    },
    Rule {
        left: "",
        grapheme: "SI",
        right: "V",
        phones: &["SH"],
    },
    Rule {
        left: "D",
        grapheme: "S",
        right: "#",
        phones: &["Z"],
    },
    Rule {
        left: "G",
        grapheme: "S",
        right: "#",
        phones: &["Z"],
    },
    Rule {
        left: "B",
        grapheme: "S",
        right: "#",
        phones: &["Z"],
    },
    Rule {
        left: "L",
        grapheme: "S",
        right: "#",
        phones: &["Z"],
    },
    Rule {
        left: "M",
        grapheme: "S",
        right: "#",
        phones: &["Z"],
    },
    Rule {
        left: "N",
        grapheme: "S",
        right: "#",
        phones: &["Z"],
    },
    Rule {
        left: "R",
        grapheme: "S",
        right: "#",
        phones: &["Z"],
    },
    Rule {
        left: "v",
        grapheme: "S",
        right: "#",
        phones: &["Z"],
    },
    Rule {
        left: "Y",
        grapheme: "S",
        right: "#",
        phones: &["Z"],
    },
    Rule {
        left: "",
        grapheme: "S",
        right: "#",
        phones: &["S"],
    },
    Rule {
        left: "",
        grapheme: "S",
        right: "",
        phones: &["S"],
    },
    // Single-consonant defaults (every letter is total).
    Rule {
        left: "",
        grapheme: "B",
        right: "",
        phones: &["B"],
    },
    Rule {
        left: "",
        grapheme: "D",
        right: "",
        phones: &["D"],
    },
    Rule {
        left: "",
        grapheme: "F",
        right: "",
        phones: &["F"],
    },
    Rule {
        left: "",
        grapheme: "H",
        right: "",
        phones: &["HH"],
    },
    Rule {
        left: "",
        grapheme: "J",
        right: "",
        phones: &["JH"],
    },
    Rule {
        left: "",
        grapheme: "K",
        right: "",
        phones: &["K"],
    },
    Rule {
        left: "",
        grapheme: "L",
        right: "",
        phones: &["L"],
    },
    Rule {
        left: "",
        grapheme: "M",
        right: "",
        phones: &["M"],
    },
    Rule {
        left: "",
        grapheme: "N",
        right: "",
        phones: &["N"],
    },
    Rule {
        left: "",
        grapheme: "P",
        right: "",
        phones: &["P"],
    },
    Rule {
        left: "",
        grapheme: "R",
        right: "",
        phones: &["R"],
    },
    Rule {
        left: "",
        grapheme: "T",
        right: "",
        phones: &["T"],
    },
    Rule {
        left: "",
        grapheme: "V",
        right: "",
        phones: &["V"],
    },
    Rule {
        left: "",
        grapheme: "W",
        right: "",
        phones: &["W"],
    },
    Rule {
        left: "#",
        grapheme: "X",
        right: "",
        phones: &["Z"],
    },
    Rule {
        left: "",
        grapheme: "X",
        right: "",
        phones: &["K", "S"],
    },
    Rule {
        left: "",
        grapheme: "Z",
        right: "",
        phones: &["Z"],
    },
    Rule {
        left: "",
        grapheme: "Q",
        right: "",
        phones: &["K"],
    },
];

/// Vowel letters for context classes: `AEIOU`, plus `Y` outside word-initial
/// position (where it is a consonant, as in `yes`).
fn is_vowel(c: char, pos: usize) -> bool {
    matches!(c, 'A' | 'E' | 'I' | 'O' | 'U') || (c == 'Y' && pos > 0)
}

/// Match one context pattern character against a letter: `V`/`C` classes or
/// a literal letter (`#` is boundary-only and handled by the caller).
/// Lowercase `c`/`v` escape the classes to match a literal `C`/`V`.
fn class_matches(pattern: char, c: char, pos: usize) -> bool {
    match pattern {
        'V' => is_vowel(c, pos),
        'C' => c.is_ascii_uppercase() && !is_vowel(c, pos),
        'c' => c == 'C',
        'v' => c == 'V',
        _ => pattern == c,
    }
}

/// Match a context pattern at `edge`: left patterns end at `edge`
/// (exclusive), right patterns start at `edge`. `#` matches only a word
/// boundary (out of range); anything else must match a letter.
fn context_matches(pattern: &str, letters: &[char], edge: usize, left: bool) -> bool {
    let pat: Vec<char> = pattern.chars().collect();
    for (k, &p) in pat.iter().enumerate() {
        let pos = if left {
            edge as isize - pat.len() as isize + k as isize
        } else {
            edge as isize + k as isize
        };
        if p == '#' {
            if pos >= 0 && (pos as usize) < letters.len() {
                return false;
            }
            continue;
        }
        if pos < 0 || (pos as usize) >= letters.len() {
            return false;
        }
        if !class_matches(p, letters[pos as usize], pos as usize) {
            return false;
        }
    }
    true
}

/// Match a grapheme literally at a position (all graphemes are ASCII).
fn grapheme_matches(grapheme: &str, letters: &[char], at: usize) -> bool {
    let g: Vec<char> = grapheme.chars().collect();
    letters.len() - at >= g.len() && letters[at..at + g.len()] == g[..]
}

/// Derive a pronunciation for `word` from the ordered rule set.
///
/// The input is uppercased and non-letters are dropped (interior
/// apostrophes from possessives vanish); the result is never empty —
/// degenerate inputs fall back to `AH` so every lexicon line keeps at
/// least one phone. Deterministic: same input, same phones.
pub fn rule_g2p(word: &str) -> Vec<String> {
    let letters: Vec<char> = word
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    let mut phones: Vec<String> = Vec::new();
    let mut i = 0;
    while i < letters.len() {
        let mut advanced = false;
        for rule in RULES {
            if !grapheme_matches(rule.grapheme, &letters, i) {
                continue;
            }
            // Graphemes are ASCII: byte length equals char length.
            let len = rule.grapheme.len();
            if !context_matches(rule.left, &letters, i, true)
                || !context_matches(rule.right, &letters, i + len, false)
            {
                continue;
            }
            phones.extend(rule.phones.iter().map(|p| (*p).to_string()));
            i += len;
            advanced = true;
            break;
        }
        if !advanced {
            // Unreachable: every letter has a default rule. Skip rather than stall.
            i += 1;
        }
    }
    if phones.is_empty() {
        phones.push("AH".to_string());
    }
    phones
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PHONEME_ALPHABET;

    fn expect(word: &str, want: &[&str]) {
        let got = rule_g2p(word);
        let want: Vec<String> = want.iter().map(|p| (*p).to_string()).collect();
        assert_eq!(got, want, "word {word}");
    }

    #[test]
    fn spot_check_common_patterns() {
        let cases: &[(&str, &[&str])] = &[
            ("CAT", &["K", "AE", "T"]),
            ("DOG", &["D", "AO", "G"]),
            ("FISH", &["F", "IH", "SH"]),
            ("JUMP", &["JH", "AH", "M", "P"]),
            ("HELLO", &["HH", "EH", "L", "OW"]),
            ("WORLD", &["W", "AO", "R", "L", "D"]),
            ("PHONE", &["F", "OW", "N"]),
            ("CHURCH", &["CH", "ER", "CH"]),
            ("THING", &["TH", "IH", "NG"]),
            ("BOX", &["B", "AA", "K", "S"]),
            ("QUIZ", &["K", "W", "IH", "Z"]),
            ("QUITE", &["K", "W", "AY", "T"]),
            ("QUIET", &["K", "W", "AY", "AH", "T"]),
            ("GUIDE", &["G", "AY", "D"]),
            ("LEAGUE", &["L", "IY", "G"]),
            ("VAGUE", &["V", "EY", "G"]),
            ("FATIGUE", &["F", "AE", "T", "IY", "G"]),
            ("SIGN", &["S", "AY", "N"]),
            ("BOTTLE", &["B", "AA", "T", "AH", "L"]),
            ("TABLE", &["T", "EY", "B", "AH", "L"]),
            ("TITLE", &["T", "AY", "T", "AH", "L"]),
            ("NOBLE", &["N", "OW", "B", "AH", "L"]),
            ("RUBLE", &["R", "UW", "B", "AH", "L"]),
            ("NATION", &["N", "EY", "SH", "AH", "N"]),
            ("VISION", &["V", "IH", "ZH", "AH", "N"]),
            ("MUSIC", &["M", "AH", "Z", "IH", "K"]),
            ("HAPPY", &["HH", "AE", "P", "IY"]),
            ("FLY", &["F", "L", "AY"]),
            ("COW", &["K", "AW"]),
            ("SNOW", &["S", "N", "OW"]),
            ("OWE", &["OW"]),
            ("NIGHT", &["N", "AY", "T"]),
            ("THROUGH", &["TH", "R", "UW"]),
            ("THOUGHT", &["TH", "AO", "T"]),
            ("TOUGH", &["T", "AH", "F"]),
            ("LAUGH", &["L", "AE", "F"]),
            ("WEIGH", &["W", "EY"]),
            ("ENOUGH", &["EH", "N", "AH", "F"]),
            ("KNOCK", &["N", "AA", "K"]),
            ("WRITE", &["R", "AY", "T"]),
            ("LAZY", &["L", "EY", "Z", "IY"]),
            ("DOGS", &["D", "AO", "G", "Z"]),
            ("CATS", &["K", "AE", "T", "S"]),
            ("BOXES", &["B", "AA", "K", "S", "AH", "Z"]),
            ("CHURCHES", &["CH", "ER", "CH", "AH", "Z"]),
            ("BUS", &["B", "AH", "S"]),
            ("WOLVES", &["W", "AA", "L", "V", "Z"]),
            ("RHUBARB", &["R", "AH", "B", "AA", "R", "B"]),
            ("MOTHER", &["M", "AH", "TH", "ER"]),
            ("FATHER", &["F", "AA", "TH", "ER"]),
            ("BUTTON", &["B", "AH", "T", "AH", "N"]),
            ("SON", &["S", "AH", "N"]),
            ("ON", &["AH", "N"]),
            ("LIE", &["L", "AY"]),
            ("FRIEND", &["F", "R", "EH", "N", "D"]),
            ("FIELD", &["F", "IY", "L", "D"]),
            ("PIECE", &["P", "IY", "S"]),
            ("OCEAN", &["AA", "SH", "N"]),
            ("PIANO", &["P", "IY", "AE", "N", "OW"]),
            ("EYE", &["EY"]),
            ("THE", &["TH"]),
            ("I", &["AY"]),
            ("O", &["OW"]),
            ("A", &["AH"]),
        ];
        for (word, want) in cases {
            expect(word, want);
        }
    }

    #[test]
    fn outputs_use_only_the_shared_inventory() {
        let sweep = [
            "CAT", "PHONE", "CHURCH", "THROUGH", "QUIET", "LEAGUE", "NATION", "VISION", "MUSIC",
            "HAPPY", "ENOUGH", "RHUBARB", "MOTHER", "BUTTON", "OCEAN", "PIANO", "STRENGTH",
            "TWELFTH", "SCHEDULE", "PSYCHIC", "MNEMONIC", "YACHT", "COLONEL",
        ];
        for word in sweep {
            for phone in rule_g2p(word) {
                assert!(
                    PHONEME_ALPHABET.contains(&phone.as_str()),
                    "word {word}: phone {phone} outside the 39-phone inventory"
                );
            }
        }
    }

    #[test]
    fn engine_is_total_and_deterministic() {
        for word in ["", "'", "123", "R2D2", "DON'T", "CAF\u{e9}"] {
            let first = rule_g2p(word);
            assert!(!first.is_empty(), "word {word:?} yielded no phones");
            assert_eq!(first, rule_g2p(word), "word {word:?} is nondeterministic");
        }
        assert_eq!(rule_g2p(""), vec!["AH".to_string()]);
        assert_eq!(rule_g2p("DON'T"), rule_g2p("DONT"));
    }

    #[test]
    fn rule_table_is_ascii() {
        for rule in RULES {
            assert!(rule.left.is_ascii(), "non-ascii left {:?}", rule.left);
            assert!(
                rule.grapheme.is_ascii(),
                "non-ascii grapheme {:?}",
                rule.grapheme
            );
            assert!(rule.right.is_ascii(), "non-ascii right {:?}", rule.right);
        }
    }
}
