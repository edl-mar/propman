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
// and Java itself checks none).
//
// Source: OpenJDK 22.0.2, 2026-11-07, via `java tools/Codes.java`. Languages are `Locale.getISOLanguages()` (ISO 639-1 plus
// the legacy codes iw, in, ji, mo) united with the languages of `Locale.getAvailableLocales()`
// (adds three-letter codes such as fil). Countries are `Locale.getISOCountries()` united with
// the two-letter countries of `Locale.getAvailableLocales()` (adds CS, DG, EA, IC), plus the
// user-assigned `XK` (Kosovo) by hand. Three-digit regions (UN M.49) are accepted without a list.
const LANGUAGES: &str = "aa ab ae af agq ak am an ann ar as asa ast av ay az ba bas be bem bez bg bgc bh bho \
    bi blo bm bn bo br brx bs ca ccp ce ceb cgg ch chr ckb co cr cs csw cu cv cy da dav \
    de dje doi dsb dua dv dyo dz ebu ee el en eo es et eu ewo fa ff fi fil fj fo fr frr \
    fur fy ga gd gl gn gsw gu guz gv ha haw he hi ho hr hsb ht hu hy hz ia id ie ig ii \
    ik in io is it iu iw ja jgo ji jmc jv ka kab kam kde kea kg kgp khq ki kj kk kkj kl \
    kln km kn ko kok kr ks ksb ksf ksh ku kv kw kxv ky la lag lb lg li lij lkt lmo ln \
    lo lrc lt lu luo luy lv mai mas mdf mer mfe mg mgh mgo mh mi mk ml mn mni mo mr ms \
    mt mua my mzn na naq nb nd nds ne ng nl nmg nn nnh no nqo nr nus nv ny nyn oc oj om \
    or os pa pcm pi pis pl prg ps pt qu raj rm rn ro rof ru rw rwk sa sah saq sat sbp \
    sc sd se seh ses sg shi si sk sl sm smn sms sn so sq sr ss st su sv sw syr szl ta \
    te teo tg th ti tk tl tn to tok tr ts tt tw twq ty tzm ug uk ur uz vai ve vec vi \
    vmw vo vun wa wae wo xh xnr xog yav yi yo yrl yue za zgh zh zu";

const COUNTRIES: &str = "AD AE AF AG AI AL AM AO AQ AR AS AT AU AW AX AZ BA BB BD BE BF BG BH BI BJ BL BM BN \
    BO BQ BR BS BT BV BW BY BZ CA CC CD CF CG CH CI CK CL CM CN CO CR CS CU CV CW CX CY \
    CZ DE DG DJ DK DM DO DZ EA EC EE EG EH ER ES ET FI FJ FK FM FO FR GA GB GD GE GF GG \
    GH GI GL GM GN GP GQ GR GS GT GU GW GY HK HM HN HR HT HU IC ID IE IL IM IN IO IQ IR \
    IS IT JE JM JO JP KE KG KH KI KM KN KP KR KW KY KZ LA LB LC LI LK LR LS LT LU LV LY \
    MA MC MD ME MF MG MH MK ML MM MN MO MP MQ MR MS MT MU MV MW MX MY MZ NA NC NE NF NG \
    NI NL NO NP NR NU NZ OM PA PE PF PG PH PK PL PM PN PR PS PT PW PY QA RE RO RS RU RW \
    SA SB SC SD SE SG SH SI SJ SK SL SM SN SO SR SS ST SV SX SY SZ TC TD TF TG TH TJ TK \
    TL TM TN TO TR TT TV TW TZ UA UG UM US UY UZ VA VC VE VG VI VN VU WF WS XK YE YT ZA \
    ZM ZW";

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
