//! Santa quote decks (ui 3.9): one remaining-index deck per language, saved
//! under `quotesLeft`, `quotesLeftJa`, ... The quote texts are in `fp-text`.

use fp_core::effect::Lang;
use fp_core::rng::Rng;

/// Pref key of the deck for a language.
pub fn key(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "quotesLeft",
        Lang::Ja => "quotesLeftJa",
        Lang::Es => "quotesLeftEs",
        Lang::De => "quotesLeftDe",
        Lang::Fr => "quotesLeftFr",
        Lang::Ko => "quotesLeftKo",
        Lang::Ru => "quotesLeftRu",
        Lang::Tr => "quotesLeftTr",
        Lang::Kk => "quotesLeftKk",
    }
}

/// Pick the next quote index for a list of `n` quotes. Drops
/// out-of-range entries, refills with `0..n` when empty, removes the
/// picked index. Returns `(index, remaining deck to save)`. An empty list
/// (`n == 0`) gives index 0 and an empty deck.
pub fn next(left: &[i64], n: usize, rng: &mut impl Rng) -> (usize, Vec<i64>) {
    let mut deck: Vec<i64> = left.iter().copied().filter(|&i| i >= 0 && (i as usize) < n).collect();
    if deck.is_empty() {
        deck = (0..n as i64).collect();
    }
    if deck.is_empty() {
        return (0, deck);
    }
    let at = rng.below(deck.len() as u64) as usize;
    let picked = deck.remove(at);
    (picked as usize, deck)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fp_core::rng::Lcg64;

    #[test]
    fn keys_cover_all_languages() {
        let keys: Vec<_> = Lang::ALL.iter().map(|l| key(*l)).collect();
        assert_eq!(keys[0], "quotesLeft");
        assert_eq!(keys[1], "quotesLeftJa");
        assert_eq!(keys[6], "quotesLeftRu");
        let mut sorted = keys.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(keys[8], "quotesLeftKk");
        assert_eq!(sorted.len(), 9);
    }

    #[test]
    fn deck_never_repeats_until_empty() {
        let mut rng = Lcg64::new(7);
        let mut deck: Vec<i64> = Vec::new();
        let mut seen = Vec::new();
        for _ in 0..5 {
            let (i, rest) = next(&deck, 5, &mut rng);
            seen.push(i);
            deck = rest;
        }
        seen.sort();
        assert_eq!(seen, vec![0, 1, 2, 3, 4]);
        assert!(deck.is_empty());
        let (i, rest) = next(&deck, 5, &mut rng);
        assert!(i < 5);
        assert_eq!(rest.len(), 4);
    }

    #[test]
    fn out_of_range_entries_are_dropped() {
        let mut rng = Lcg64::new(1);
        let (i, rest) = next(&[-3, 9, 2, 100], 4, &mut rng);
        assert_eq!(i, 2);
        assert!(rest.is_empty());
        let (i, rest) = next(&[9], 3, &mut rng);
        assert!(i < 3);
        assert_eq!(rest.len(), 2);
    }

    #[test]
    fn empty_list_is_safe() {
        let mut rng = Lcg64::new(1);
        assert_eq!(next(&[], 0, &mut rng), (0, vec![]));
    }
}
