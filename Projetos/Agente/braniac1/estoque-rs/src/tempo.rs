//! Datas sem dependencias: dia = dias desde 1970-01-01 no fuso da loja (Brasil, UTC-3, sem horario de verao).
use std::time::{SystemTime, UNIX_EPOCH};

const FUSO_S: i64 = -3 * 3600;

fn agora_epoch() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

pub fn hoje_dias() -> i64 {
    (agora_epoch() + FUSO_S).div_euclid(86400)
}

pub fn dias_de_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

pub fn civil_de_dias(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// "2026-11-02" ou "02/11/2026" -> dias. None se a data nao existe.
pub fn parse_data(s: &str) -> Option<i64> {
    let s = s.trim();
    let p: Vec<&str> = s.split(|c| c == '-' || c == '/').collect();
    if p.len() != 3 {
        return None;
    }
    let n: Vec<i64> = p.iter().map(|x| x.trim().parse().ok()).collect::<Option<_>>()?;
    let (y, m, d) = if p[0].len() == 4 { (n[0], n[1], n[2]) } else { (n[2], n[1], n[0]) };
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || !(2000..=2200).contains(&y) {
        return None;
    }
    let z = dias_de_civil(y, m, d);
    (civil_de_dias(z) == (y, m, d)).then_some(z) // rejeita 31/02
}

pub fn fmt_data(dias: i64) -> String {
    let (y, m, d) = civil_de_dias(dias);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Carimbo "AAAA-MM-DD HH:MM:SS" no fuso da loja.
pub fn iso_agora() -> String {
    let t = agora_epoch() + FUSO_S;
    let s = t.rem_euclid(86400);
    format!("{} {:02}:{:02}:{:02}", fmt_data(t.div_euclid(86400)), s / 3600, (s % 3600) / 60, s % 60)
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn ida_e_volta() {
        for z in [-1, 0, 365, 19000, 20000, 20454] {
            let (y, m, d) = civil_de_dias(z);
            assert_eq!(dias_de_civil(y, m, d), z);
        }
        assert_eq!(parse_data("2026-11-02"), parse_data("02/11/2026"));
        assert_eq!(fmt_data(parse_data("2026-11-02").unwrap()), "2026-11-02");
    }

    #[test]
    fn data_invalida() {
        assert!(parse_data("2026-02-31").is_none());
        assert!(parse_data("amanha").is_none());
        assert!(parse_data("2026-13-01").is_none());
    }
}
