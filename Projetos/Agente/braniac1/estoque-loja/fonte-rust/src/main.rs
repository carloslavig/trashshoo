//! estoque.exe --db <arquivo> <comando>   (JSON em stdin, JSON em stdout; sempre {"ok":true,...} ou {"ok":false,"erro":"..."})
mod compras;
mod estoque;
mod tempo;

use estoque::{Estoque, Falha, R};
use serde_json::{json, Value};
use std::io::{Read, Write};

fn executar(e: &mut Estoque, cmd: &str, v: &Value) -> R<Value> {
    match cmd {
        "item" => e.item(v),
        "receita" => e.receita(v),
        "entrada" => e.entrada(v),
        "saida" => e.saida(v),
        "ajuste" => e.ajuste(v),
        "estorno" => e.estorno(v),
        "contar" => e.contar(v),
        "reconciliar" => e.reconciliar(v),
        "pedido" => e.consumir_pedido(v),
        "saldos" => e.saldos(),
        "alertas" => e.alertas(v),
        "movimentos" => e.movimentos(v),
        "compras" => e.compras(v),
        "indisponiveis" => e.indisponiveis(),
        "backup" => e.backup(v),
        "conferir" => e.conferir(),
        "versao" => Ok(json!({"versao": env!("CARGO_PKG_VERSION")})),
        _ => Err(Falha(format!("Comando desconhecido: {cmd}"))),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut db = "estoque.db".to_string();
    let mut cmd = String::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--db" && i + 1 < args.len() {
            db = args[i + 1].clone();
            i += 1;
        } else if cmd.is_empty() {
            cmd = args[i].clone();
        }
        i += 1;
    }
    if cmd.is_empty() {
        eprintln!("uso: estoque --db <arquivo> <comando>   (JSON no stdin)");
        std::process::exit(2);
    }
    let mut entrada = String::new();
    let _ = std::io::stdin().read_to_string(&mut entrada);
    let saida = match serde_json::from_str::<Value>(if entrada.trim().is_empty() { "{}" } else { &entrada }) {
        Err(e) => json!({"ok": false, "erro": format!("JSON invalido: {e}")}),
        Ok(v) => match Estoque::abrir(&db).and_then(|mut e| executar(&mut e, &cmd, &v)) {
            Ok(mut r) => {
                if let Some(o) = r.as_object_mut() {
                    o.insert("ok".into(), json!(true));
                }
                r
            }
            Err(e) => json!({"ok": false, "erro": e.0}),
        },
    };
    let _ = writeln!(std::io::stdout(), "{saida}");
}
