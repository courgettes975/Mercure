//! Montants en centimes (i64). Aucun flottant.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(transparent)]
// Un montant en centimes d'euro. Sérialisé en JSON comme un simple entier.
pub struct Money(pub i64);

/// Arrondi au plus proche, demi vers l'extérieur de zéro (arrondi commercial).
pub fn div_round(num: i128, den: i128) -> i128 {
    assert!(den > 0);
    let q = num / den;
    let r = num % den;
    if r.abs() * 2 >= den {
        q + num.signum()
    } else {
        q
    }
}

impl Money {
    // Zéro, pour les sommes et valeurs par défaut.
    pub const ZERO: Money = Money(0);

    // Valeur brute en centimes.
    pub fn cents(self) -> i64 {
        self.0
    }

    /// quantité exprimée en millièmes (1,5 -> 1500) x prix unitaire.
    pub fn times_milli_qty(self, qty_milli: i64) -> Money {
        Money(div_round(self.0 as i128 * qty_milli as i128, 1000) as i64)
    }

    /// Montant d'un temps : minutes / 60 x taux horaire.
    pub fn for_minutes(hourly_rate: Money, minutes: i64) -> Money {
        Money(div_round(hourly_rate.0 as i128 * minutes as i128, 60) as i64)
    }

    /// TVA d'un montant HT pour un taux en points de base (2000 = 20 %).
    pub fn vat(self, rate_bp: i64) -> Money {
        Money(div_round(self.0 as i128 * rate_bp as i128, 10_000) as i64)
    }

    /// Format français : "1 234,56 €" (espace insécable comme séparateur de milliers).
    pub fn format_fr(self) -> String {
        // Signe traité à part, le reste du calcul se fait sur la valeur absolue.
        let neg = self.0 < 0;
        let abs = self.0.unsigned_abs();
        let euros = (abs / 100).to_string();
        let mut grouped = String::new();
        // Insertion d'un séparateur tous les trois chiffres en partant de la droite.
        for (i, c) in euros.chars().enumerate() {
            if i > 0 && (euros.len() - i).is_multiple_of(3) {
                grouped.push('\u{a0}');
            }
            grouped.push(c);
        }
        format!("{}{},{:02}\u{a0}€", if neg { "-" } else { "" }, grouped, abs % 100)
    }

    /// Lecture d'une saisie : "1234,5", "1 234.50", "80" -> centimes.
    pub fn parse(input: &str) -> Option<Money> {
        // Nettoyage : espaces, symbole euro et espace insécable sont ignorés.
        let s: String = input
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '€' && *c != '\u{a0}')
            .collect();
        if s.is_empty() {
            return None;
        }
        // Signe négatif éventuel.
        let (neg, s) = match s.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, s.as_str()),
        };
        // Virgule française ou point : les deux sont acceptés comme séparateur décimal.
        let s = s.replace(',', ".");
        let mut parts = s.splitn(2, '.');
        let int_part = parts.next()?;
        let frac_part = parts.next().unwrap_or("");
        if int_part.is_empty() && frac_part.is_empty() {
            return None;
        }
        // Uniquement des chiffres, et au plus deux décimales (les centimes).
        if !int_part.chars().all(|c| c.is_ascii_digit())
            || !frac_part.chars().all(|c| c.is_ascii_digit())
            || frac_part.len() > 2
        {
            return None;
        }
        let euros: i64 = if int_part.is_empty() { 0 } else { int_part.parse().ok()? };
        // "5" après la virgule vaut 50 centimes, "05" en vaut 5.
        let cents: i64 = match frac_part.len() {
            0 => 0,
            1 => frac_part.parse::<i64>().ok()? * 10,
            _ => frac_part.parse().ok()?,
        };
        // checked_* : un montant absurde ne provoque pas de débordement silencieux.
        let total = euros.checked_mul(100)?.checked_add(cents)?;
        Some(Money(if neg { -total } else { total }))
    }
}

// Opérateurs + et - et somme d'itérateur, pour écrire les totaux naturellement.
impl std::ops::Add for Money {
    type Output = Money;
    fn add(self, o: Money) -> Money {
        Money(self.0 + o.0)
    }
}

impl std::ops::Sub for Money {
    type Output = Money;
    fn sub(self, o: Money) -> Money {
        Money(self.0 - o.0)
    }
}

impl std::iter::Sum for Money {
    fn sum<I: Iterator<Item = Money>>(iter: I) -> Money {
        Money(iter.map(|m| m.0).sum())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temps_7h_a_80_euros() {
        assert_eq!(Money::for_minutes(Money(8000), 7 * 60), Money(56000));
    }

    #[test]
    fn temps_arrondi_au_centime() {
        // 50 min à 80 € = 66,666... -> 66,67 €
        assert_eq!(Money::for_minutes(Money(8000), 50), Money(6667));
    }

    #[test]
    fn exemple_mission_autocad() {
        let heures = Money::for_minutes(Money(8000), 14 * 60);
        let total = heures + Money(12000) + Money(9500);
        assert_eq!(total, Money(133500));
    }

    #[test]
    fn tva_20_pourcent() {
        assert_eq!(Money(56000).vat(2000), Money(11200));
        assert_eq!(Money(333).vat(2000), Money(67)); // 66,6 -> 67
        assert_eq!(Money(56000).vat(0), Money(0));
    }

    #[test]
    fn quantite_decimale() {
        assert_eq!(Money(8000).times_milli_qty(1500), Money(12000));
        assert_eq!(Money(333).times_milli_qty(3000), Money(999));
    }

    #[test]
    fn arrondi_negatif_symetrique() {
        assert_eq!(div_round(-5, 10), -1);
        assert_eq!(div_round(5, 10), 1);
        assert_eq!(div_round(-4, 10), 0);
    }

    #[test]
    fn format_francais() {
        assert_eq!(Money(133500).format_fr(), "1\u{a0}335,00\u{a0}€");
        assert_eq!(Money(5).format_fr(), "0,05\u{a0}€");
        assert_eq!(Money(-123456789).format_fr(), "-1\u{a0}234\u{a0}567,89\u{a0}€");
    }

    #[test]
    fn lecture_saisie() {
        assert_eq!(Money::parse("80"), Some(Money(8000)));
        assert_eq!(Money::parse("1 234,5"), Some(Money(123450)));
        assert_eq!(Money::parse("1234.56 €"), Some(Money(123456)));
        assert_eq!(Money::parse("-3,1"), Some(Money(-310)));
        assert_eq!(Money::parse("12,345"), None);
        assert_eq!(Money::parse("abc"), None);
        assert_eq!(Money::parse(""), None);
    }
}
