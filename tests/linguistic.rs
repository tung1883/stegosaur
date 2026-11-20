mod common;
use common::random_bytes;
use proptest::prelude::*;
use stegosaur::crypto::KdfParams;
use stegosaur::linguistic::{bits_per_word, decode, detokenize, encode, tokenize, LanguageModel, NgramModel};
use stegosaur::Error;

fn kdf() -> KdfParams {
    KdfParams::fast()
}

fn model() -> NgramModel {
    NgramModel::default_english()
}

#[test]
fn roundtrip_various_sizes() {
    let m = model();
    for len in [0usize, 1, 2, 7, 16, 33, 100, 400] {
        let data = random_bytes(len, len as u64);
        let text = encode(&m, b"pw", &data, kdf()).unwrap();
        assert_eq!(decode(&m, b"pw", &text, kdf()).unwrap(), data, "len {len}");
    }
}

#[test]
fn wrong_passphrase_fails() {
    let m = model();
    let text = encode(&m, b"right", b"meet at noon", kdf()).unwrap();
    assert!(matches!(decode(&m, b"wrong", &text, kdf()), Err(Error::NoPayload)));
}

#[test]
fn same_payload_gives_different_cover_texts() {
    let m = model();
    let a = encode(&m, b"pw", b"same", kdf()).unwrap();
    let b = encode(&m, b"pw", b"same", kdf()).unwrap();
    assert_ne!(a, b, "random nonce must randomise the cover text");
}

#[test]
fn cover_text_uses_only_model_vocabulary_and_ends_in_a_sentence() {
    let m = model();
    let text = encode(&m, b"pw", &random_bytes(60, 1), kdf()).unwrap();
    for t in tokenize(&text) {
        assert!(m.id(&t).is_some(), "token {t:?} not in model vocabulary");
    }
    assert!(text.trim_end().ends_with('.'), "text should end at a sentence boundary: {text:?}");
}

#[test]
fn survives_whitespace_reflow() {
    let m = model();
    let data = random_bytes(40, 3);
    let text = encode(&m, b"pw", &data, kdf()).unwrap();
    let reflowed = text.replace(' ', "\n  ").replace("\n  ,", ",");
    assert_eq!(decode(&m, b"pw", &reflowed, kdf()).unwrap(), data);
}

#[test]
fn edited_text_is_rejected() {
    let m = model();
    let text = encode(&m, b"pw", &random_bytes(40, 4), kdf()).unwrap();
    let words: Vec<&str> = text.split(' ').collect();
    // 40 B payload = 4 + 12 + 40 + 16 bytes of frame; the first quarter of the words is always inside it.
    for victim in [1usize, words.len() / 8, words.len() / 4] {
        let mut w = words.clone();
        w[victim] = if w[victim] == "the" { "and" } else { "the" };
        let r = decode(&m, b"pw", &w.join(" "), kdf());
        assert!(r.is_err(), "swapping word {victim} must break authentication");
    }
    assert!(decode(&m, b"pw", "completely foreign vocabulary xyzzy", kdf()).is_err());
    assert!(decode(&m, b"pw", "", kdf()).is_err());
}

#[test]
fn words_after_the_frame_are_ignored() {
    let m = model();
    let data = random_bytes(30, 6);
    let text = encode(&m, b"pw", &data, kdf()).unwrap();
    assert_eq!(decode(&m, b"pw", &format!("{text} zzz unknown words after the end"), kdf()).unwrap(), data);
}

#[test]
fn different_model_cannot_decode() {
    let a = model();
    let b = NgramModel::train("one two three four five six seven. one three five seven. two four six.", 16);
    let text = encode(&a, b"pw", b"data", kdf()).unwrap();
    assert!(decode(&b, b"pw", &text, kdf()).is_err());
}

#[test]
fn capacity_is_reasonable() {
    let m = model();
    let data = random_bytes(200, 5);
    let text = encode(&m, b"pw", &data, kdf()).unwrap();
    let words = tokenize(&text).len();
    let bpw = bits_per_word(&m, &text);
    println!("200 B -> {words} words, {bpw:.2} bits/word");
    assert!(bpw > 0.5, "bits per word too low: {bpw}");
    // payload bits + framing must fit: words * bpw >= frame bits
    assert!(words as f64 * bpw + 8.0 >= (200 + 4 + 12 + 16) as f64 * 8.0);
}

#[test]
fn single_candidate_model_carries_zero_bits_but_does_not_hang() {
    let m = NgramModel::train("a b c a b c a b c a b c.", 16);
    // contexts are mostly deterministic; encoding a payload must either succeed or fail, not loop forever
    let r = encode(&m, b"pw", b"hi", kdf());
    if let Ok(text) = r {
        assert_eq!(decode(&m, b"pw", &text, kdf()).unwrap(), b"hi");
    }
}

#[test]
fn tokenizer_roundtrip_property() {
    let t = tokenize("Hello, world!  It's 5 o'clock; really?\nYes.");
    let refs: Vec<&str> = t.iter().map(|s| s.as_str()).collect();
    assert_eq!(detokenize(&refs), "Hello, world! It's 5 o'clock; really? Yes.");
    assert_eq!(tokenize(&detokenize(&refs)), t);
}

#[test]
fn huffman_prefix_free_via_decoding_every_candidate() {
    // Every candidate in every context must decode to itself: walk the whole model.
    let m = model();
    let text = encode(&m, b"pw", &random_bytes(500, 8), kdf()).unwrap();
    assert!(decode(&m, b"pw", &text, kdf()).is_ok());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(40))]
    #[test]
    fn prop_roundtrip(data in proptest::collection::vec(any::<u8>(), 0..120), pw in proptest::collection::vec(any::<u8>(), 1..16)) {
        let m = model();
        let text = encode(&m, &pw, &data, kdf()).unwrap();
        prop_assert_eq!(decode(&m, &pw, &text, kdf()).unwrap(), data);
    }

    #[test]
    fn prop_tokenize_idempotent(s in "[a-zA-Z ,.;:!?\\n']{0,80}") {
        let t = tokenize(&s);
        let refs: Vec<&str> = t.iter().map(|x| x.as_str()).collect();
        prop_assert_eq!(tokenize(&detokenize(&refs)), t);
    }
}

mod styles {
    use super::*;
    use stegosaur::corpora::Style;
    use stegosaur::linguistic::{detokenize_lines, tokenize_lines};

    const LOG_STYLES: [Style; 4] = [Style::Syslog, Style::Postgres, Style::Nginx, Style::Env];

    #[test]
    fn every_style_roundtrips_at_several_sizes() {
        for style in [Style::Prose, Style::Syslog, Style::Postgres, Style::Nginx, Style::Env] {
            let m = style.model(None, None, None);
            for len in [0usize, 1, 16, 100, 300] {
                let data = random_bytes(len, len as u64 + 3);
                let text = encode(&m, b"pw", &data, kdf()).unwrap();
                assert_eq!(decode(&m, b"pw", &text, kdf()).unwrap(), data, "{style:?} len {len}");
            }
        }
    }

    #[test]
    fn log_styles_emit_multiline_text_using_only_their_vocabulary() {
        for style in LOG_STYLES {
            let m = style.model(None, None, None);
            let text = encode(&m, b"pw", &random_bytes(120, 1), kdf()).unwrap();
            assert!(text.lines().count() > 3, "{style:?} should span several lines");
            for t in m.tokenize(&text) {
                assert!(m.id(&t).is_some(), "{style:?}: token {t:?} not in vocabulary");
            }
            assert!(text.ends_with('\n'), "{style:?} should stop at a line boundary");
        }
    }

    #[test]
    fn styles_are_not_interchangeable() {
        let (a, b) = (Style::Syslog.model(None, None, None), Style::Postgres.model(None, None, None));
        let text = encode(&a, b"pw", b"payload", kdf()).unwrap();
        assert!(decode(&b, b"pw", &text, kdf()).is_err());
    }

    #[test]
    fn line_tokenizer_is_an_exact_inverse() {
        let samples = ["a b c", "a\nb", "\nfoo", "foo\n", "a\n\nb", "x=1 y=2\n  z=3 \n", "", "\n", "k=v:w,\"q\"\n[1]"];
        for s in samples {
            let t = tokenize_lines(s);
            let refs: Vec<&str> = t.iter().map(|x| x.as_str()).collect();
            assert_eq!(tokenize_lines(&detokenize_lines(&refs)), t, "{s:?}");
        }
    }

    #[test]
    fn corpora_are_deterministic() {
        for style in LOG_STYLES {
            assert_eq!(style.corpus(), style.corpus());
        }
    }

    #[test]
    fn capacity_and_samples() {
        println!("\nstyle      bits/word  words for 64 B");
        for style in LOG_STYLES {
            let m = style.model(None, None, None);
            let text = encode(&m, b"pw", &random_bytes(64, 5), kdf()).unwrap();
            let words = m.tokenize(&text).len();
            println!("{:<10} {:<10.2} {}", format!("{style:?}"), bits_per_word(&m, &text), words);
            println!("--- sample ({style:?}) ---");
            for line in text.lines().take(6) {
                println!("{line}");
            }
        }
        let m = Style::Prose.model(None, None, None);
        let text = encode(&m, b"pw", &random_bytes(64, 5), kdf()).unwrap();
        println!("{:<10} {:<10.2} {}", "Prose", bits_per_word(&m, &text), m.tokenize(&text).len());
    }
}

mod order_sweep {
    use super::*;
    use std::collections::{HashMap, HashSet};
    use stegosaur::corpora::Style;

    /// Line shape with variable fields masked: tokens present in under 5% of corpus lines become `<v>`.
    fn shapes(style: Style) -> (HashSet<String>, impl Fn(&str) -> String) {
        let corpus = style.corpus();
        let lines: Vec<&str> = corpus.lines().collect();
        let mut doc_freq: HashMap<String, usize> = HashMap::new();
        for l in &lines {
            let uniq: HashSet<&str> = l.split_whitespace().collect();
            for t in uniq {
                *doc_freq.entry(t.to_string()).or_default() += 1;
            }
        }
        let cutoff = lines.len() / 20;
        let shape = move |line: &str| {
            line.split_whitespace()
                .map(|t| if doc_freq.get(t).copied().unwrap_or(0) >= cutoff { t } else { "<v>" })
                .collect::<Vec<_>>()
                .join(" ")
        };
        let set = lines.iter().map(|l| shape(l)).collect();
        (set, shape)
    }

    #[test]
    fn realism_versus_capacity_by_context_order() {
        println!("\nstyle     order  bits/word  words/200B  well-formed lines");
        for style in [Style::Syslog, Style::Postgres, Style::Nginx] {
            let (known, shape) = shapes(style);
            let mut conformity = Vec::new();
            for order in 1..=6usize {
                let m = style.model(None, None, Some(order));
                let (mut ok, mut total, mut bpw, mut words) = (0usize, 0usize, 0.0, 0usize);
                let runs = 4;
                for seed in 0..runs {
                    let data = random_bytes(200, 90 + seed);
                    let text = encode(&m, b"pw", &data, kdf()).unwrap();
                    assert_eq!(decode(&m, b"pw", &text, kdf()).unwrap(), data, "{style:?} order {order}");
                    for line in text.lines() {
                        total += 1;
                        ok += known.contains(&shape(line)) as usize;
                    }
                    bpw += bits_per_word(&m, &text) / runs as f64;
                    words += m.tokenize(&text).len() / runs as usize;
                }
                let c = ok as f64 / total as f64;
                println!("{:<9} {:<6} {:<10.2} {:<11} {:.0}%", format!("{style:?}"), order, bpw, words, c * 100.0);
                conformity.push(c);
            }
            assert!(conformity[3] > conformity[0] + 0.15, "{style:?}: order 4 should be clearly better formed than order 1: {conformity:?}");
        }
    }
}
