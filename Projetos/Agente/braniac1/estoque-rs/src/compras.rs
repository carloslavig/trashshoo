//! Previsao de compras: funcao pura (sem I/O). Especificacao do ticket T027 do NexoDev (Otto).
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Urgencia {
    Agora,
    Hoje,
    Semana,
}

impl Urgencia {
    pub fn nome(self) -> &'static str {
        match self {
            Urgencia::Agora => "agora",
            Urgencia::Hoje => "hoje",
            Urgencia::Semana => "semana",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sugestao {
    pub item: String,
    pub saldo: f64,
    pub dias_restantes: Option<f64>,
    pub comprar: f64,
    pub urgencia: Urgencia,
}

/// Itens com folga (>= 7 dias, acima do minimo) ficam de fora: a lista de compras so mostra o que importa.
pub fn previsao(consumo_diario: &[(String, f64)], saldo: &[(String, f64)], minimo: &[(String, f64)], dias_cobertura: f64) -> Vec<Sugestao> {
    let cons: HashMap<&str, f64> = consumo_diario.iter().map(|(n, v)| (n.as_str(), *v)).collect();
    let mini: HashMap<&str, f64> = minimo.iter().map(|(n, v)| (n.as_str(), *v)).collect();
    let mut out = Vec::new();
    for (nome, s) in saldo {
        let c = cons.get(nome.as_str()).copied().unwrap_or(0.0).max(0.0);
        let m = mini.get(nome.as_str()).copied().unwrap_or(0.0);
        let dias = (c > 0.0).then(|| s / c);
        let mut comprar = (c * dias_cobertura - s).max(0.0);
        if *s < m {
            comprar = comprar.max(m - s);
        }
        let urg = match dias {
            _ if *s <= 0.0 => Some(Urgencia::Agora),
            Some(d) if d < 1.0 => Some(Urgencia::Agora),
            Some(d) if d < 2.0 => Some(Urgencia::Hoje),
            Some(d) if d < 7.0 => Some(Urgencia::Semana),
            _ if *s < m => Some(Urgencia::Semana),
            _ => None,
        };
        if let Some(urgencia) = urg {
            out.push(Sugestao { item: nome.clone(), saldo: *s, dias_restantes: dias, comprar, urgencia });
        }
    }
    out.sort_by(|a, b| {
        let da = a.dias_restantes.unwrap_or(f64::MAX);
        let db = b.dias_restantes.unwrap_or(f64::MAX);
        a.urgencia.cmp(&b.urgencia).then(da.total_cmp(&db)).then(a.item.cmp(&b.item))
    });
    out
}

#[cfg(test)]
mod testes {
    use super::*;

    fn v(x: &[(&str, f64)]) -> Vec<(String, f64)> {
        x.iter().map(|(n, f)| (n.to_string(), *f)).collect()
    }

    #[test]
    fn consumo_zero_so_aparece_abaixo_do_minimo() {
        let r = previsao(&v(&[]), &v(&[("sal", 5.0), ("oleo", 1.0)]), &v(&[("oleo", 3.0)]), 3.0);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].item, "oleo");
        assert_eq!(r[0].comprar, 2.0);
        assert_eq!(r[0].dias_restantes, None);
    }

    #[test]
    fn saldo_negativo_ou_zero_e_agora() {
        let r = previsao(&v(&[("queijo", 2.0)]), &v(&[("queijo", -1.0)]), &v(&[]), 3.0);
        assert_eq!(r[0].urgencia, Urgencia::Agora);
        assert_eq!(r[0].comprar, 7.0);
    }

    #[test]
    fn ordena_por_urgencia_e_dias() {
        let r = previsao(
            &v(&[("a", 1.0), ("b", 1.0), ("c", 1.0)]),
            &v(&[("a", 5.0), ("b", 0.5), ("c", 1.5)]),
            &v(&[]),
            3.0,
        );
        let nomes: Vec<&str> = r.iter().map(|s| s.item.as_str()).collect();
        assert_eq!(nomes, ["b", "c", "a"]);
        assert_eq!(r[1].urgencia, Urgencia::Hoje);
        assert_eq!(r[2].urgencia, Urgencia::Semana);
    }
}
