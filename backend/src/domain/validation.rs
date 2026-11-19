//! Contrôles de format réutilisables.

// Contrôle simple : une arobase, un domaine avec un point, pas d'espace. Pas de vérification DNS.
pub fn is_valid_email(s: &str) -> bool {
    let s = s.trim();
    let Some((local, domain)) = s.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !s.contains(char::is_whitespace)
        && s.matches('@').count() == 1
}

// Retire les espaces d'un numéro saisi "944 642 917".
fn digits_only(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Clé de Luhn (SIREN / SIRET).
fn luhn_ok(digits: &str) -> bool {
    let mut sum = 0;
    for (i, c) in digits.chars().rev().enumerate() {
        let Some(mut d) = c.to_digit(10) else { return false };
        if !i.is_multiple_of(2) {
            d *= 2;
            if d > 9 {
                d -= 9;
            }
        }
        sum += d;
    }
    sum % 10 == 0
}

// SIREN : 9 chiffres + clé de Luhn.
pub fn is_valid_siren(s: &str) -> bool {
    let d = digits_only(s);
    d.len() == 9 && d.chars().all(|c| c.is_ascii_digit()) && luhn_ok(&d)
}

/// SIRET : 14 chiffres, clé de Luhn (exception connue La Poste non gérée).
pub fn is_valid_siret(s: &str) -> bool {
    let d = digits_only(s);
    d.len() == 14 && d.chars().all(|c| c.is_ascii_digit()) && luhn_ok(&d)
}

/// IBAN : contrôle modulo 97.
pub fn is_valid_iban(s: &str) -> bool {
    let iban: String = s.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_uppercase();
    if iban.len() < 15 || iban.len() > 34 || !iban.chars().all(|c| c.is_ascii_alphanumeric()) {
        return false;
    }
    // Les 4 premiers caractères passent à la fin, les lettres deviennent des nombres (A=10...),
    // puis reste de la division par 97 calculé chiffre par chiffre pour éviter un très grand entier.
    let rearranged = format!("{}{}", &iban[4..], &iban[..4]);
    let mut rem: u32 = 0;
    for c in rearranged.chars() {
        let v = if c.is_ascii_digit() {
            c.to_digit(10).unwrap()
        } else {
            c as u32 - 'A' as u32 + 10
        };
        let chunk = if v >= 10 { 100 } else { 10 };
        rem = (rem * chunk + v) % 97;
    }
    rem == 1
}

// Espaces multiples et bords supprimés : "  Jean   Martin " -> "Jean Martin".
pub fn normalize_spaces(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Date au format AAAA-MM-JJ.
pub fn parse_date(s: &str) -> Option<chrono::NaiveDate> {
    chrono::NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok()
}

/// Heure "HH:MM" -> minutes depuis minuit.
pub fn parse_hhmm(s: &str) -> Option<i64> {
    let (h, m) = s.trim().split_once(':')?;
    let (h, m): (i64, i64) = (h.parse().ok()?, m.parse().ok()?);
    ((0..24).contains(&h) && (0..60).contains(&m) && s.trim().len() == 5).then_some(h * 60 + m)
}

/// Texte de recherche : minuscules, sans accents, espaces simples.
pub fn search_key(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    // Chaque caractère accentué est remplacé par son équivalent simple.
    for c in s.chars().flat_map(|c| c.to_lowercase()) {
        let r = match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => "a",
            'ç' => "c",
            'è' | 'é' | 'ê' | 'ë' => "e",
            'ì' | 'í' | 'î' | 'ï' => "i",
            'ñ' => "n",
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' => "o",
            'ù' | 'ú' | 'û' | 'ü' => "u",
            'ý' | 'ÿ' => "y",
            'œ' => "oe",
            'æ' => "ae",
            _ => {
                out.push(c);
                continue;
            }
        };
        out.push_str(r);
    }
    normalize_spaces(&out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emails() {
        assert!(is_valid_email("jean.martin@demo-industrie.fr"));
        assert!(!is_valid_email("jean.martin"));
        assert!(!is_valid_email("a@b"));
        assert!(!is_valid_email("a b@c.fr"));
    }

    #[test]
    fn siren_siret() {
        assert!(is_valid_siren("944642917"));
        assert!(is_valid_siren("944 642 917"));
        assert!(!is_valid_siren("944642918"));
        assert!(is_valid_siret("94464291700016"));
        assert!(!is_valid_siret("94464291700017"));
        assert!(!is_valid_siret("123"));
    }

    #[test]
    fn dates_et_heures() {
        assert!(parse_date("2027-01-12").is_some());
        assert!(parse_date("2027-02-30").is_none());
        assert_eq!(parse_hhmm("09:00"), Some(540));
        assert_eq!(parse_hhmm("17:30"), Some(1050));
        assert_eq!(parse_hhmm("9:00"), None);
        assert_eq!(parse_hhmm("24:00"), None);
    }

    #[test]
    fn cle_de_recherche() {
        assert_eq!(search_key("  LOPEZ  Éléonore "), "lopez eleonore");
        assert_eq!(search_key("Œuvre Ça"), "oeuvre ca");
    }

    #[test]
    fn iban() {
        assert!(is_valid_iban("FR76 3000 6000 0112 3456 7890 189"));
        assert!(!is_valid_iban("FR76 3000 6000 0112 3456 7890 188"));
    }
}
