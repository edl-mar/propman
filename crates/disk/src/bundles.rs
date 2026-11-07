//! Assigns each `.properties` file to a bundle and locale, based on its name and its siblings.
//!
//! A bundle is (folder, base name); bundles never span folders and evidence from other
//! folders is ignored. Pure: only looks at paths, never touches the file system. The result
//! does not depend on the order of the input.
//!
//! Rules, in short (details in `crates/disk/tests/bundles.rs`):
//! - The suffix is the longest well-formed Java locale (`de`, `de_DE`, `zh_Hans_CN`,
//!   `de_DE_POSIX`) at the end of the stem; canonical case (`de`, `Hans`, `DE`) is preferred.
//! - A suffix is evidence-backed if a suffix-less file with the base name exists in the
//!   same folder, or another file shares the base name. A known language/country is a hint.
//! - Not well-formed, or well-formed but unknown without evidence: the file is a default file
//!   and its whole stem is the bundle name.
//! - A file whose stem is the base name of another file is a default file.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

// Language and country codes are only a hint, never a condition (the lists change over time
// and Java itself checks none). Written from memory on 2026-11-07: ISO 639-1 and ISO 3166-1
// alpha-2, plus legacy Java language codes (iw, in, ji), a few common three-letter languages
// and the user-assigned `XK` (Kosovo). Compare with the JDK's `Locale.getISOLanguages()` and
// `Locale.getISOCountries()` before relying on completeness.
const LANGUAGES: &str = "aa ab ae af ak am an ar as av ay az ba be bg bh bi bm bn bo br bs ca ce \
    ch co cr cs cu cv cy da de dv dz ee el en eo es et eu fa ff fi fj fo fr fy ga gd gl gn gu gv \
    ha he hi ho hr ht hu hy hz ia id ie ig ii ik io is it iu ja jv ka kg ki kj kk kl km kn ko kr \
    ks ku kv kw ky la lb lg li ln lo lt lu lv mg mh mi mk ml mn mr ms mt my na nb nd ne ng nl nn \
    no nr nv ny oc oj om or os pa pi pl ps pt qu rm rn ro ru rw sa sc sd se sg si sk sl sm sn so \
    sq sr ss st su sv sw ta te tg th ti tk tl tn to tr ts tt tw ty ug uk ur uz ve vi vo wa wo xh \
    yi yo za zh zu iw in ji fil ceb haw yue gsw ast kab";

const COUNTRIES: &str = "AD AE AF AG AI AL AM AO AQ AR AS AT AU AW AX AZ BA BB BD BE BF BG BH BI \
    BJ BL BM BN BO BQ BR BS BT BV BW BY BZ CA CC CD CF CG CH CI CK CL CM CN CO CR CU CV CW CX CY \
    CZ DE DJ DK DM DO DZ EC EE EG EH ER ES ET FI FJ FK FM FO FR GA GB GD GE GF GG GH GI GL GM GN \
    GP GQ GR GS GT GU GW GY HK HM HN HR HT HU ID IE IL IM IN IO IQ IR IS IT JE JM JO JP KE KG KH \
    KI KM KN KP KR KW KY KZ LA LB LC LI LK LR LS LT LU LV LY MA MC MD ME MF MG MH MK ML MM MN MO \
    MP MQ MR MS MT MU MV MW MX MY MZ NA NC NE NF NG NI NL NO NP NR NU NZ OM PA PE PF PG PH PK PL \
    PM PN PR PS PT PW PY QA RE RO RS RU RW SA SB SC SD SE SG SH SI SJ SK SL SM SN SO SR SS ST SV \
    SX SY SZ TC TD TF TG TH TJ TK TL TM TN TO TR TT TV TW TZ UA UG UM US UY UZ VA VC VE VG VI VN \
    VU WF WS YE YT ZA ZM ZW XK";

/// `lang[_script][_COUNTRY[_variant]]` has at most four parts.
const MAX_SUFFIX_PARTS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    Certain,
    Uncertain,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classified {
    pub path: PathBuf,
    /// Base name; together with the file's folder this identifies the bundle.
    pub bundle: String,
    /// Locale as written in the file name (`de`, `de_DE`); `None` for the default file.
    pub locale: Option<String>,
    pub confidence: Confidence,
}

fn contains(list: &str, code: &str) -> bool {
    list.split_whitespace().any(|c| c == code)
}

fn is_alphabetic(s: &str) -> bool {
    s.bytes().all(|b| b.is_ascii_alphabetic())
}

struct Shape {
    canonical: bool,
    known: bool,
}

/// Parses `lang[_script][_COUNTRY[_variant]]`; `None` if the parts do not form exactly that.
fn parse_suffix(parts: &[&str]) -> Option<Shape> {
    let (&lang, mut rest) = parts.split_first()?;
    if !(is_alphabetic(lang) && (2..=3).contains(&lang.len())) {
        return None;
    }
    let mut canonical = lang.bytes().all(|b| b.is_ascii_lowercase());

    if let Some((&script, tail)) = rest.split_first()
        && script.len() == 4
        && is_alphabetic(script)
    {
        canonical &= script[..1].bytes().all(|b| b.is_ascii_uppercase())
            && script[1..].bytes().all(|b| b.is_ascii_lowercase());
        rest = tail;
    }

    let mut country = None;
    if let Some((&c, tail)) = rest.split_first() {
        let letters = c.len() == 2 && is_alphabetic(c);
        let digits = c.len() == 3 && c.bytes().all(|b| b.is_ascii_digit());
        if letters || digits {
            canonical &= !c.bytes().any(|b| b.is_ascii_lowercase());
            country = Some(c);
            rest = tail;
        }
    }

    if country.is_some()
        && let Some((&variant, tail)) = rest.split_first()
        && variant.bytes().all(|b| b.is_ascii_alphanumeric())
        && !variant.is_empty()
    {
        rest = tail;
    }
    if !rest.is_empty() {
        return None;
    }

    let known = contains(LANGUAGES, &lang.to_ascii_lowercase())
        && country.is_none_or(|c| {
            c.bytes().all(|b| b.is_ascii_digit()) || contains(COUNTRIES, &c.to_ascii_uppercase())
        });
    Some(Shape { canonical, known })
}

struct Candidate {
    base: String,
    locale: String,
    canonical: bool,
    known: bool,
}

/// The longest well-formed locale suffix of `stem`, canonical case preferred over any case.
/// The base name must not be empty.
fn candidate(stem: &str) -> Option<Candidate> {
    let parts: Vec<&str> = stem.split('_').collect();
    for require_canonical in [true, false] {
        for len in (1..=MAX_SUFFIX_PARTS).rev() {
            if len >= parts.len() {
                continue;
            }
            let (base, suffix) = parts.split_at(parts.len() - len);
            let base = base.join("_");
            if !base.is_empty()
                && let Some(shape) = parse_suffix(suffix)
                && (shape.canonical || !require_canonical)
            {
                return Some(Candidate {
                    base,
                    locale: suffix.join("_"),
                    canonical: shape.canonical,
                    known: shape.known,
                });
            }
        }
    }
    None
}

struct Entry<'a> {
    path: &'a Path,
    folder: Option<&'a Path>,
    stem: String,
    candidate: Option<Candidate>,
}

/// Returns one entry per input path, sorted by path.
pub fn classify_files(paths: &[PathBuf]) -> Vec<Classified> {
    let mut sorted: Vec<&Path> = paths.iter().map(PathBuf::as_path).collect();
    sorted.sort();
    sorted.dedup();

    let entries: Vec<Entry> = sorted
        .into_iter()
        .map(|path| {
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            Entry {
                path,
                folder: path.parent(),
                candidate: candidate(&stem),
                stem,
            }
        })
        .collect();

    // Evidence never crosses folders: facts are collected per folder.
    let mut stems: HashSet<(Option<&Path>, &str)> = HashSet::new();
    let mut base_counts: HashMap<(Option<&Path>, &str), usize> = HashMap::new();
    for e in &entries {
        stems.insert((e.folder, &e.stem));
        if let Some(c) = &e.candidate {
            *base_counts.entry((e.folder, &c.base)).or_default() += 1;
        }
    }

    entries
        .iter()
        .map(|e| {
            let default_file = || Classified {
                path: e.path.to_path_buf(),
                bundle: e.stem.clone(),
                locale: None,
                confidence: Confidence::Certain,
            };
            let Some(c) = &e.candidate else {
                return default_file();
            };
            // Another file uses this stem as its base name: this is that bundle's default file.
            if base_counts.contains_key(&(e.folder, e.stem.as_str())) {
                return default_file();
            }
            let evidence = stems.contains(&(e.folder, c.base.as_str()))
                || base_counts
                    .get(&(e.folder, c.base.as_str()))
                    .is_some_and(|&n| n >= 2);
            let confidence = match (evidence, c.known) {
                (true, true) if c.canonical => Confidence::Certain,
                (false, false) => return default_file(),
                _ => Confidence::Uncertain,
            };
            Classified {
                path: e.path.to_path_buf(),
                bundle: c.base.clone(),
                locale: Some(c.locale.clone()),
                confidence,
            }
        })
        .collect()
}
