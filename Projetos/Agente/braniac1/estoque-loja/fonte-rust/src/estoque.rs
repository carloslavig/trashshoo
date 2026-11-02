//! Estoque da loja: itens, lotes com validade (FEFO), historico IMUTAVEL de movimentos, receitas e alertas.
//! Quantidades guardadas em milesimos (inteiro) para nunca acumular erro de ponto flutuante.
//! Ideias vindas do estoque da clinica: unidade-base x unidade de compra, saldo por lote, FEFO, estorno, contagem que gera ajuste.
use crate::compras;
use crate::tempo;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

pub struct Falha(pub String);
impl From<rusqlite::Error> for Falha {
    fn from(e: rusqlite::Error) -> Self {
        Falha(e.to_string())
    }
}
impl From<String> for Falha {
    fn from(e: String) -> Self {
        Falha(e)
    }
}
impl From<&str> for Falha {
    fn from(e: &str) -> Self {
        Falha(e.to_string())
    }
}
pub type R<T> = Result<T, Falha>;

const SCHEMA: &str = "
PRAGMA journal_mode=WAL;
PRAGMA foreign_keys=ON;
CREATE TABLE IF NOT EXISTS items(
  id INTEGER PRIMARY KEY, nome TEXT NOT NULL, nome_norm TEXT NOT NULL UNIQUE,
  unidade TEXT NOT NULL DEFAULT 'un', unidade_compra TEXT, fator_mil INTEGER,
  minimo INTEGER NOT NULL DEFAULT 0, precisa_validade INTEGER NOT NULL DEFAULT 0,
  validade_dias INTEGER, ativo INTEGER NOT NULL DEFAULT 1);
CREATE TABLE IF NOT EXISTS lotes(
  id INTEGER PRIMARY KEY, item_id INTEGER NOT NULL REFERENCES items(id), codigo TEXT NOT NULL,
  validade INTEGER, saldo INTEGER NOT NULL CHECK(saldo>=0), UNIQUE(item_id,codigo));
CREATE TABLE IF NOT EXISTS movimentos(
  id INTEGER PRIMARY KEY, ts TEXT NOT NULL, dia INTEGER NOT NULL,
  item_id INTEGER NOT NULL REFERENCES items(id), lote_id INTEGER NOT NULL REFERENCES lotes(id),
  tipo TEXT NOT NULL, qtd INTEGER NOT NULL, motivo TEXT, pedido TEXT,
  estorno_de INTEGER REFERENCES movimentos(id), fefo_override INTEGER NOT NULL DEFAULT 0);
CREATE TRIGGER IF NOT EXISTS mov_sem_update BEFORE UPDATE ON movimentos BEGIN SELECT RAISE(ABORT,'historico imutavel'); END;
CREATE TRIGGER IF NOT EXISTS mov_sem_delete BEFORE DELETE ON movimentos BEGIN SELECT RAISE(ABORT,'historico imutavel'); END;
CREATE TABLE IF NOT EXISTS receitas(
  produto_norm TEXT NOT NULL, produto TEXT NOT NULL, item_id INTEGER NOT NULL REFERENCES items(id),
  qtd INTEGER NOT NULL CHECK(qtd>0), PRIMARY KEY(produto_norm,item_id));
CREATE TABLE IF NOT EXISTS baixas(pedido TEXT PRIMARY KEY, ts TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS contagens(
  id INTEGER PRIMARY KEY, item_id INTEGER NOT NULL, lote_id INTEGER NOT NULL, contado INTEGER NOT NULL,
  sistema INTEGER NOT NULL, status TEXT NOT NULL DEFAULT 'PENDENTE', mov_id INTEGER, ts TEXT NOT NULL);
";

pub struct Estoque {
    pub c: Connection,
    pub hoje: i64,
}

struct Item {
    id: i64,
    nome: String,
    unidade: String,
    fator_mil: Option<i64>,
    minimo: i64,
    precisa_validade: bool,
    validade_dias: Option<i64>,
}

pub fn norm(s: &str) -> String {
    let mut o = String::new();
    for ch in s.chars() {
        match ch {
            'á' | 'à' | 'â' | 'ã' | 'ä' | 'Á' | 'À' | 'Â' | 'Ã' | 'Ä' => o.push('a'),
            'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => o.push('e'),
            'í' | 'ì' | 'î' | 'ï' | 'Í' | 'Ì' | 'Î' | 'Ï' => o.push('i'),
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' | 'Ó' | 'Ò' | 'Ô' | 'Õ' | 'Ö' => o.push('o'),
            'ú' | 'ù' | 'û' | 'ü' | 'Ú' | 'Ù' | 'Û' | 'Ü' => o.push('u'),
            'ç' | 'Ç' => o.push('c'),
            '-' | '_' => o.push(' '),
            ',' => o.push('.'),
            c => o.extend(c.to_lowercase()),
        }
    }
    o.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn mil(x: f64) -> i64 {
    (x * 1000.0).round() as i64
}
pub fn fl(i: i64) -> f64 {
    i as f64 / 1000.0
}

fn txt(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(|x| x.as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}
fn req_txt(v: &Value, k: &str) -> R<String> {
    txt(v, k).ok_or_else(|| Falha(format!("Falta o campo '{k}'")))
}
fn num(v: &Value, k: &str) -> Option<f64> {
    v.get(k).and_then(|x| x.as_f64().or_else(|| x.as_str().and_then(|s| s.trim().replace(',', ".").parse().ok())))
}
fn req_num(v: &Value, k: &str) -> R<f64> {
    num(v, k).filter(|n| n.is_finite()).ok_or_else(|| Falha(format!("Falta o numero '{k}'")))
}
fn flag(v: &Value, k: &str) -> Option<bool> {
    v.get(k).and_then(|x| x.as_bool())
}

fn achar_item(c: &Connection, nome: &str) -> R<Item> {
    c.query_row(
        "SELECT id,nome,unidade,fator_mil,minimo,precisa_validade,validade_dias FROM items WHERE nome_norm=?1 AND ativo=1",
        params![norm(nome)],
        |r| Ok(Item { id: r.get(0)?, nome: r.get(1)?, unidade: r.get(2)?, fator_mil: r.get(3)?, minimo: r.get(4)?, precisa_validade: r.get::<_, i64>(5)? != 0, validade_dias: r.get(6)? }),
    )
    .optional()?
    .ok_or_else(|| Falha(format!("Item '{nome}' nao esta cadastrado")))
}

impl Estoque {
    pub fn abrir(caminho: &str) -> R<Self> {
        Self::preparar(Connection::open(caminho)?)
    }

    pub fn memoria() -> R<Self> {
        Self::preparar(Connection::open_in_memory()?)
    }

    fn preparar(c: Connection) -> R<Self> {
        c.execute_batch(SCHEMA)?;
        Ok(Self { c, hoje: tempo::hoje_dias() })
    }

    fn mov(&self, item: i64, lote: i64, tipo: &str, qtd: i64, motivo: &str, pedido: Option<&str>, estorno_de: Option<i64>, fefo: bool) -> R<i64> {
        self.c.execute(
            "INSERT INTO movimentos(ts,dia,item_id,lote_id,tipo,qtd,motivo,pedido,estorno_de,fefo_override) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![tempo::iso_agora(), self.hoje, item, lote, tipo, qtd, motivo, pedido, estorno_de, fefo as i64],
        )?;
        Ok(self.c.last_insert_rowid())
    }

    fn em_transacao<T>(&mut self, f: impl FnOnce(&Self) -> R<T>) -> R<T> {
        self.c.execute_batch("BEGIN IMMEDIATE")?;
        match f(self) {
            Ok(v) => {
                self.c.execute_batch("COMMIT")?;
                Ok(v)
            }
            Err(e) => {
                let _ = self.c.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }

    // ---------- cadastro ----------
    pub fn item(&mut self, v: &Value) -> R<Value> {
        let nome = req_txt(v, "nome")?;
        let unidade = txt(v, "unidade").unwrap_or_else(|| "un".into());
        let unidade_compra = txt(v, "unidade_compra");
        let fator = num(v, "fator");
        if unidade_compra.is_some() && fator.map_or(true, |f| f <= 0.0) {
            return Err("unidade_compra exige 'fator' maior que zero (ex.: 1 caixa = 12 un)".into());
        }
        let minimo = num(v, "minimo").unwrap_or(0.0);
        if minimo < 0.0 {
            return Err("minimo nao pode ser negativo".into());
        }
        let validade_dias = num(v, "validade_dias").map(|d| d as i64).filter(|d| *d > 0);
        let precisa = flag(v, "precisa_validade").unwrap_or(validade_dias.is_some());
        self.c.execute(
            "INSERT INTO items(nome,nome_norm,unidade,unidade_compra,fator_mil,minimo,precisa_validade,validade_dias,ativo) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,1)
             ON CONFLICT(nome_norm) DO UPDATE SET nome=?1,unidade=?3,unidade_compra=?4,fator_mil=?5,minimo=?6,precisa_validade=?7,validade_dias=?8,ativo=1",
            params![nome, norm(&nome), unidade, unidade_compra, fator.map(mil), mil(minimo), precisa as i64, validade_dias],
        )?;
        let it = achar_item(&self.c, &nome)?;
        Ok(json!({"id": it.id, "nome": it.nome}))
    }

    pub fn receita(&mut self, v: &Value) -> R<Value> {
        let produto = req_txt(v, "produto")?;
        let insumos = v.get("insumos").and_then(|x| x.as_array()).ok_or("Falta a lista 'insumos'")?;
        self.em_transacao(|s| {
            s.c.execute("DELETE FROM receitas WHERE produto_norm=?1", params![norm(&produto)])?;
            for i in insumos {
                let it = achar_item(&s.c, &req_txt(i, "item")?)?;
                let q = req_num(i, "quantidade")?;
                if q <= 0.0 {
                    return Err("quantidade da receita deve ser maior que zero".into());
                }
                s.c.execute("INSERT INTO receitas(produto_norm,produto,item_id,qtd) VALUES(?1,?2,?3,?4) ON CONFLICT(produto_norm,item_id) DO UPDATE SET qtd=qtd+?4", params![norm(&produto), produto, it.id, mil(q)])?;
            }
            Ok(json!({"produto": produto, "insumos": insumos.len()}))
        })
    }

    // ---------- movimentos ----------
    pub fn entrada(&mut self, v: &Value) -> R<Value> {
        let it = achar_item(&self.c, &req_txt(v, "item")?)?;
        let mut q = req_num(v, "quantidade")?;
        if q <= 0.0 {
            return Err("Quantidade de entrada deve ser maior que zero".into());
        }
        if flag(v, "em_unidade_compra").unwrap_or(false) {
            q *= fl(it.fator_mil.ok_or("Este item nao tem unidade de compra cadastrada")?);
        }
        let validade = match txt(v, "validade") {
            Some(s) => Some(tempo::parse_data(&s).ok_or("Validade invalida (use AAAA-MM-DD ou DD/MM/AAAA)")?),
            None => it.validade_dias.map(|d| self.hoje + d),
        };
        if it.precisa_validade && validade.is_none() {
            return Err(format!("'{}' precisa de validade: informe 'validade' ou cadastre 'validade_dias'", it.nome).into());
        }
        let codigo = txt(v, "lote").unwrap_or_else(|| if it.precisa_validade { format!("E{}", tempo::fmt_data(self.hoje).replace('-', "")) } else { "SEM-LOTE".into() });
        let motivo = txt(v, "motivo").unwrap_or_else(|| "Entrada".into());
        let q_mil = mil(q);
        self.em_transacao(|s| {
            s.c.execute(
                "INSERT INTO lotes(item_id,codigo,validade,saldo) VALUES(?1,?2,?3,0) ON CONFLICT(item_id,codigo) DO UPDATE SET validade=COALESCE(?3,validade)",
                params![it.id, codigo, validade],
            )?;
            let lote: i64 = s.c.query_row("SELECT id FROM lotes WHERE item_id=?1 AND codigo=?2", params![it.id, codigo], |r| r.get(0))?;
            s.c.execute("UPDATE lotes SET saldo=saldo+?1 WHERE id=?2", params![q_mil, lote])?;
            let m = s.mov(it.id, lote, "ENTRADA", q_mil, &motivo, None, None, false)?;
            Ok(json!({"mov": m, "item": it.nome, "lote": codigo, "quantidade": fl(q_mil), "unidade": it.unidade}))
        })
    }

    /// Baixa em ordem FEFO (vence primeiro, sai primeiro). Lote vencido nao serve para consumo (so para dar baixa por vencimento).
    fn baixar_fefo(&self, item: i64, mut qtd: i64, tipo: &str, motivo: &str, pedido: Option<&str>, incluir_vencidos: bool) -> R<i64> {
        let lotes: Vec<(i64, i64)> = {
            let mut st = self.c.prepare(
                "SELECT id,saldo FROM lotes WHERE item_id=?1 AND saldo>0 AND (validade IS NULL OR validade>=?2 OR ?3=1) ORDER BY validade IS NULL, validade, id",
            )?;
            let linhas = st.query_map(params![item, self.hoje, incluir_vencidos as i64], |r| Ok((r.get(0)?, r.get(1)?)))?;
            linhas.collect::<Result<_, _>>()?
        };
        let mut feito = 0;
        for (id, saldo) in lotes {
            if qtd == 0 {
                break;
            }
            let t = qtd.min(saldo);
            self.c.execute("UPDATE lotes SET saldo=saldo-?1 WHERE id=?2", params![t, id])?;
            self.mov(item, id, tipo, -t, motivo, pedido, None, false)?;
            qtd -= t;
            feito += t;
        }
        Ok(feito)
    }

    fn disponivel(&self, item: i64, incluir_vencidos: bool) -> R<i64> {
        Ok(self.c.query_row(
            "SELECT COALESCE(SUM(saldo),0) FROM lotes WHERE item_id=?1 AND (validade IS NULL OR validade>=?2 OR ?3=1)",
            params![item, self.hoje, incluir_vencidos as i64],
            |r| r.get(0),
        )?)
    }

    pub fn saida(&mut self, v: &Value) -> R<Value> {
        let it = achar_item(&self.c, &req_txt(v, "item")?)?;
        let q = mil(req_num(v, "quantidade")?);
        if q <= 0 {
            return Err("Quantidade de saida deve ser maior que zero".into());
        }
        let tipo = txt(v, "tipo").unwrap_or_else(|| "CONSUMO".into()).to_uppercase();
        if !["CONSUMO", "PERDA", "VENCIMENTO"].contains(&tipo.as_str()) {
            return Err("tipo de saida deve ser CONSUMO, PERDA ou VENCIMENTO".into());
        }
        let motivo = txt(v, "motivo").unwrap_or_default();
        let venc = tipo == "VENCIMENTO";
        self.em_transacao(|s| {
            if let Some(cod) = txt(v, "lote") {
                let (id, saldo): (i64, i64) = s.c.query_row("SELECT id,saldo FROM lotes WHERE item_id=?1 AND codigo=?2", params![it.id, cod], |r| Ok((r.get(0)?, r.get(1)?))).optional()?.ok_or("Lote nao encontrado")?;
                if saldo < q {
                    return Err(format!("Estoque insuficiente no lote {cod}: tem {}", fl(saldo)).into());
                }
                let primeiro: Option<i64> = s.c.query_row("SELECT id FROM lotes WHERE item_id=?1 AND saldo>0 AND (validade IS NULL OR validade>=?2) ORDER BY validade IS NULL, validade, id LIMIT 1", params![it.id, s.hoje], |r| r.get(0)).optional()?;
                s.c.execute("UPDATE lotes SET saldo=saldo-?1 WHERE id=?2", params![q, id])?;
                let m = s.mov(it.id, id, &tipo, -q, &motivo, None, None, primeiro.is_some_and(|p| p != id))?;
                return Ok(json!({"mov": m, "item": it.nome, "quantidade": fl(q)}));
            }
            let tem = s.disponivel(it.id, venc)?;
            if tem < q {
                return Err(format!("Estoque insuficiente de {}: tem {} {}, pediu {}", it.nome, fl(tem), it.unidade, fl(q)).into());
            }
            s.baixar_fefo(it.id, q, &tipo, &motivo, None, venc)?;
            Ok(json!({"item": it.nome, "quantidade": fl(q), "unidade": it.unidade, "saldo": fl(s.disponivel(it.id, true)?)}))
        })
    }

    pub fn ajuste(&mut self, v: &Value) -> R<Value> {
        let it = achar_item(&self.c, &req_txt(v, "item")?)?;
        let cod = req_txt(v, "lote")?;
        let delta = mil(req_num(v, "delta")?);
        if delta == 0 {
            return Err("Ajuste nao pode ter delta zero".into());
        }
        let motivo = txt(v, "motivo").ok_or("Ajuste manual exige motivo")?;
        self.em_transacao(|s| {
            let id = s.lote_id(it.id, &cod)?;
            s.aplicar_ajuste(it.id, id, delta, &motivo)
        })
    }

    fn lote_id(&self, item: i64, cod: &str) -> R<i64> {
        self.c.query_row("SELECT id FROM lotes WHERE item_id=?1 AND codigo=?2", params![item, cod], |r| r.get(0)).optional()?.ok_or_else(|| Falha("Lote nao encontrado".into()))
    }

    fn aplicar_ajuste(&self, item: i64, lote: i64, delta: i64, motivo: &str) -> R<Value> {
        let saldo: i64 = self.c.query_row("SELECT saldo FROM lotes WHERE id=?1", params![lote], |r| r.get(0))?;
        if saldo + delta < 0 {
            return Err("Ajuste deixaria o saldo do lote negativo".into());
        }
        self.c.execute("UPDATE lotes SET saldo=saldo+?1 WHERE id=?2", params![delta, lote])?;
        let m = self.mov(item, lote, "AJUSTE", delta, motivo, None, None, false)?;
        Ok(json!({"mov": m, "saldo_lote": fl(saldo + delta)}))
    }

    pub fn estorno(&mut self, v: &Value) -> R<Value> {
        let id = req_num(v, "mov")? as i64;
        let motivo = txt(v, "motivo").unwrap_or_default();
        self.em_transacao(|s| {
            let (item, lote, tipo, qtd): (i64, i64, String, i64) = s.c.query_row("SELECT item_id,lote_id,tipo,qtd FROM movimentos WHERE id=?1", params![id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).optional()?.ok_or("Movimentacao nao encontrada")?;
            if tipo == "ESTORNO" {
                return Err("Nao e possivel estornar um estorno".into());
            }
            let ja: Option<i64> = s.c.query_row("SELECT id FROM movimentos WHERE estorno_de=?1", params![id], |r| r.get(0)).optional()?;
            if ja.is_some() {
                return Err("Esta movimentacao ja foi estornada".into());
            }
            let saldo: i64 = s.c.query_row("SELECT saldo FROM lotes WHERE id=?1", params![lote], |r| r.get(0))?;
            if saldo - qtd < 0 {
                return Err("Estorno deixaria o saldo do lote negativo".into());
            }
            s.c.execute("UPDATE lotes SET saldo=saldo-?1 WHERE id=?2", params![qtd, lote])?;
            let m = s.mov(item, lote, "ESTORNO", -qtd, &if motivo.is_empty() { format!("Estorno da movimentacao {id}") } else { motivo.clone() }, None, Some(id), false)?;
            Ok(json!({"mov": m, "estorno_de": id}))
        })
    }

    // ---------- inventario ----------
    pub fn contar(&mut self, v: &Value) -> R<Value> {
        let it = achar_item(&self.c, &req_txt(v, "item")?)?;
        let cod = txt(v, "lote").unwrap_or_else(|| "SEM-LOTE".into());
        let contado = mil(req_num(v, "contado")?);
        if contado < 0 {
            return Err("Quantidade contada nao pode ser negativa".into());
        }
        self.em_transacao(|s| {
            let lote = s.lote_id(it.id, &cod)?;
            let sistema: i64 = s.c.query_row("SELECT saldo FROM lotes WHERE id=?1", params![lote], |r| r.get(0))?;
            s.c.execute("INSERT INTO contagens(item_id,lote_id,contado,sistema,ts) VALUES(?1,?2,?3,?4,?5)", params![it.id, lote, contado, sistema, tempo::iso_agora()])?;
            Ok(json!({"id": s.c.last_insert_rowid(), "sistema": fl(sistema), "contado": fl(contado), "diferenca": fl(contado - sistema)}))
        })
    }

    pub fn reconciliar(&mut self, v: &Value) -> R<Value> {
        let id = req_num(v, "id")? as i64;
        self.em_transacao(|s| {
            let (item, lote, contado, status): (i64, i64, i64, String) = s.c.query_row("SELECT item_id,lote_id,contado,status FROM contagens WHERE id=?1", params![id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).optional()?.ok_or("Contagem nao encontrada")?;
            if status != "PENDENTE" {
                return Err("Esta contagem ja foi reconciliada".into());
            }
            let atual: i64 = s.c.query_row("SELECT saldo FROM lotes WHERE id=?1", params![lote], |r| r.get(0))?;
            let delta = contado - atual;
            let mov = if delta != 0 { Some(s.aplicar_ajuste(item, lote, delta, &format!("Contagem #{id}"))?["mov"].as_i64().unwrap_or(0)) } else { None };
            s.c.execute("UPDATE contagens SET status='RECONCILIADA', mov_id=?1 WHERE id=?2", params![mov, id])?;
            Ok(json!({"id": id, "ajuste": fl(delta), "mov": mov}))
        })
    }

    // ---------- pedidos (baixa automatica pela receita) ----------
    fn receita_de(&self, nome: &str) -> R<Vec<(i64, i64)>> {
        let mut st = self.c.prepare("SELECT item_id,qtd FROM receitas WHERE produto_norm=?1")?;
        let linhas = st.query_map(params![norm(nome)], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(linhas.collect::<Result<_, _>>()?)
    }

    /// "2x Frango, Carne e Queijo (sem cebola)" -> [(2,"frango"),(1,"carne e queijo")]. O texto do detalhe vem do site/cliente: so serve para achar nomes de receitas.
    fn pedacos(detalhe: &str) -> Vec<(i64, String)> {
        let mut out = Vec::new();
        for p in detalhe.split(|c| c == ',' || c == ';' || c == '+' || c == '/' || c == '\n') {
            let p = p.trim();
            let (n, resto) = match p.split_once(|c: char| c == 'x' || c == 'X') {
                Some((a, b)) if !a.is_empty() && a.trim().chars().all(|c| c.is_ascii_digit()) => (a.trim().parse().unwrap_or(1), b.trim()),
                _ => (1, p),
            };
            let resto = resto.split('(').next().unwrap_or("").trim();
            if !resto.is_empty() {
                out.push((n.clamp(1, 99), resto.to_string()));
            }
        }
        out
    }

    pub fn consumir_pedido(&mut self, v: &Value) -> R<Value> {
        let pedido = req_txt(v, "pedido")?;
        let itens = v.get("itens").and_then(|x| x.as_array()).ok_or("Falta a lista 'itens'")?;
        self.em_transacao(|s| {
            let ja: Option<String> = s.c.query_row("SELECT ts FROM baixas WHERE pedido=?1", params![pedido], |r| r.get(0)).optional()?;
            if ja.is_some() {
                return Ok(json!({"ja_baixado": true, "pedido": pedido}));
            }
            let mut precisa: BTreeMap<i64, i64> = BTreeMap::new();
            let mut sem_receita: Vec<String> = Vec::new();
            for it in itens {
                let nome = txt(it, "nome").unwrap_or_default();
                let q = num(it, "qtd").unwrap_or(1.0).clamp(1.0, 999.0) as i64;
                let mut achou = false;
                for (iid, rq) in s.receita_de(&nome)? {
                    *precisa.entry(iid).or_insert(0) += rq * q;
                    achou = true;
                }
                let direto = achou; // combo sem receita propria (ex.: a bebida) e avisado mesmo que os sabores do detalhe tenham baixado
                for (n, parte) in Self::pedacos(&txt(it, "detalhe").unwrap_or_default()) {
                    for (iid, rq) in s.receita_de(&parte)? {
                        *precisa.entry(iid).or_insert(0) += rq * n * q;
                        achou = true;
                    }
                }
                if !direto && !nome.is_empty() {
                    sem_receita.push(nome);
                }
            }
            let (mut baixados, mut faltas) = (Vec::new(), Vec::new());
            for (iid, q) in precisa {
                let nome: (String, String) = s.c.query_row("SELECT nome,unidade FROM items WHERE id=?1", params![iid], |r| Ok((r.get(0)?, r.get(1)?)))?;
                let feito = s.baixar_fefo(iid, q, "CONSUMO", "Pedido", Some(&pedido), false)?;
                baixados.push(json!({"item": nome.0, "quantidade": fl(feito), "unidade": nome.1}));
                if feito < q {
                    faltas.push(json!({"item": nome.0, "faltou": fl(q - feito), "unidade": nome.1}));
                }
            }
            s.c.execute("INSERT INTO baixas(pedido,ts) VALUES(?1,?2)", params![pedido, tempo::iso_agora()])?;
            Ok(json!({"pedido": pedido, "baixados": baixados, "faltas": faltas, "sem_receita": sem_receita, "indisponiveis": s.indisponiveis_lista()?}))
        })
    }

    /// Produto com receita e algum insumo que nao cobre uma unidade (descontado o vencido) = indisponivel.
    fn indisponiveis_lista(&self) -> R<Vec<Value>> {
        let mut st = self.c.prepare(
            "SELECT r.produto, i.nome, r.qtd, COALESCE((SELECT SUM(saldo) FROM lotes l WHERE l.item_id=i.id AND (l.validade IS NULL OR l.validade>=?1)),0)
             FROM receitas r JOIN items i ON i.id=r.item_id WHERE i.ativo=1 ORDER BY r.produto",
        )?;
        let linhas: Vec<(String, String, i64, i64)> = st.query_map(params![self.hoje], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<Result<_, _>>()?;
        let mut por: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (p, ins, rq, saldo) in linhas {
            if saldo < rq {
                por.entry(p).or_default().push(ins);
            }
        }
        Ok(por.into_iter().map(|(p, m)| json!({"produto": p, "falta": m})).collect())
    }

    pub fn indisponiveis(&self) -> R<Value> {
        Ok(json!({"indisponiveis": self.indisponiveis_lista()?}))
    }

    // ---------- consultas ----------
    pub fn saldos(&self) -> R<Value> {
        let mut st = self.c.prepare("SELECT id,nome,unidade,unidade_compra,fator_mil,minimo FROM items WHERE ativo=1 ORDER BY nome_norm")?;
        let its: Vec<(i64, String, String, Option<String>, Option<i64>, i64)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?.collect::<Result<_, _>>()?;
        let mut out = Vec::new();
        for (id, nome, un, uc, fator, minimo) in its {
            let mut sl = self.c.prepare("SELECT codigo,validade,saldo FROM lotes WHERE item_id=?1 AND saldo>0 ORDER BY validade IS NULL, validade, id")?;
            let lotes: Vec<(String, Option<i64>, i64)> = sl.query_map(params![id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
            let total: i64 = lotes.iter().map(|l| l.2).sum();
            let util: i64 = lotes.iter().filter(|l| l.1.map_or(true, |d| d >= self.hoje)).map(|l| l.2).sum();
            out.push(json!({
                "item": nome, "unidade": un, "total": fl(total), "utilizavel": fl(util), "minimo": fl(minimo),
                "abaixo_minimo": minimo > 0 && util <= minimo, "unidade_compra": uc, "fator": fator.map(fl),
                "lotes": lotes.iter().map(|l| json!({"lote": l.0, "validade": l.1.map(tempo::fmt_data), "saldo": fl(l.2), "vencido": l.1.is_some_and(|d| d < self.hoje)})).collect::<Vec<_>>(),
            }));
        }
        Ok(json!({"itens": out}))
    }

    pub fn alertas(&self, v: &Value) -> R<Value> {
        let janela = num(v, "dias_vencimento").unwrap_or(3.0) as i64;
        let baixo: Vec<Value> = self.saldos()?["itens"].as_array().map(|a| a.iter().filter(|i| i["abaixo_minimo"] == true).map(|i| json!({"item": i["item"], "utilizavel": i["utilizavel"], "minimo": i["minimo"], "unidade": i["unidade"]})).collect()).unwrap_or_default();
        let mut st = self.c.prepare(
            "SELECT i.nome,l.codigo,l.validade,l.saldo,i.unidade FROM lotes l JOIN items i ON i.id=l.item_id
             WHERE l.saldo>0 AND l.validade IS NOT NULL AND l.validade<=?1 ORDER BY l.validade",
        )?;
        let venc: Vec<Value> = st
            .query_map(params![self.hoje + janela], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?, r.get::<_, String>(4)?)))?
            .map(|x| x.map(|(n, c, d, s, u)| json!({"item": n, "lote": c, "validade": tempo::fmt_data(d), "dias": d - self.hoje, "saldo": fl(s), "unidade": u, "vencido": d < self.hoje})))
            .collect::<Result<_, _>>()?;
        Ok(json!({"abaixo_minimo": baixo, "vencendo": venc, "indisponiveis": self.indisponiveis_lista()?}))
    }

    pub fn movimentos(&self, v: &Value) -> R<Value> {
        let lim = num(v, "limite").unwrap_or(50.0).clamp(1.0, 500.0) as i64;
        let filtro = txt(v, "item").map(|n| norm(&n));
        let mut st = self.c.prepare(
            "SELECT m.id,m.ts,i.nome,l.codigo,m.tipo,m.qtd,m.motivo,m.pedido,m.estorno_de,m.fefo_override,i.unidade FROM movimentos m
             JOIN items i ON i.id=m.item_id JOIN lotes l ON l.id=m.lote_id WHERE (?1 IS NULL OR i.nome_norm=?1) ORDER BY m.id DESC LIMIT ?2",
        )?;
        let linhas: Vec<Value> = st
            .query_map(params![filtro, lim], |r| {
                Ok(json!({"id": r.get::<_, i64>(0)?, "ts": r.get::<_, String>(1)?, "item": r.get::<_, String>(2)?, "lote": r.get::<_, String>(3)?, "tipo": r.get::<_, String>(4)?,
                    "quantidade": fl(r.get::<_, i64>(5)?), "motivo": r.get::<_, Option<String>>(6)?, "pedido": r.get::<_, Option<String>>(7)?, "estorno_de": r.get::<_, Option<i64>>(8)?,
                    "fefo_override": r.get::<_, i64>(9)? != 0, "unidade": r.get::<_, String>(10)?}))
            })?
            .collect::<Result<_, _>>()?;
        Ok(json!({"movimentos": linhas}))
    }

    pub fn compras(&self, v: &Value) -> R<Value> {
        let cob = num(v, "dias_cobertura").unwrap_or(3.0);
        let janela = num(v, "janela").unwrap_or(14.0).max(1.0) as i64;
        let (mut cons, mut saldo, mut mini) = (Vec::new(), Vec::new(), Vec::new());
        let mut info: BTreeMap<String, (String, Option<String>, Option<f64>)> = BTreeMap::new();
        let mut st = self.c.prepare(
            "SELECT i.nome,i.unidade,i.unidade_compra,i.fator_mil,i.minimo,
              COALESCE((SELECT SUM(saldo) FROM lotes l WHERE l.item_id=i.id AND (l.validade IS NULL OR l.validade>=?1)),0),
              COALESCE((SELECT -SUM(m.qtd) FROM movimentos m WHERE m.item_id=i.id AND m.dia>?2 AND (m.tipo='CONSUMO' OR (m.tipo='ESTORNO' AND m.estorno_de IN (SELECT id FROM movimentos WHERE tipo='CONSUMO')))),0)
             FROM items i WHERE i.ativo=1",
        )?;
        let linhas: Vec<(String, String, Option<String>, Option<i64>, i64, i64, i64)> =
            st.query_map(params![self.hoje, self.hoje - janela], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)))?.collect::<Result<_, _>>()?;
        for (n, un, uc, fator, minimo, util, gasto) in linhas {
            cons.push((n.clone(), fl(gasto) / janela as f64));
            saldo.push((n.clone(), fl(util)));
            mini.push((n.clone(), fl(minimo)));
            info.insert(n, (un, uc, fator.map(fl)));
        }
        let sug: Vec<Value> = compras::previsao(&cons, &saldo, &mini, cob)
            .into_iter()
            .map(|s| {
                let (un, uc, fator) = info.get(&s.item).cloned().unwrap_or_default();
                let mut o = Map::new();
                o.insert("item".into(), json!(s.item));
                o.insert("saldo".into(), json!(s.saldo));
                o.insert("dias_restantes".into(), json!(s.dias_restantes.map(|d| (d * 10.0).round() / 10.0)));
                o.insert("comprar".into(), json!((s.comprar * 1000.0).ceil() / 1000.0));
                o.insert("unidade".into(), json!(un));
                if let (Some(u), Some(f)) = (uc, fator) {
                    o.insert("comprar_em".into(), json!({"unidade": u, "quantidade": (s.comprar / f).ceil()}));
                }
                o.insert("urgencia".into(), json!(s.urgencia.nome()));
                Value::Object(o)
            })
            .collect();
        Ok(json!({"dias_cobertura": cob, "janela_dias": janela, "sugestoes": sug}))
    }

    pub fn backup(&self, v: &Value) -> R<Value> {
        let destino = req_txt(v, "para")?;
        self.c.execute("VACUUM INTO ?1", params![destino])?;
        Ok(json!({"arquivo": destino}))
    }

    /// Confere que o saldo de cada lote bate com a soma dos movimentos (o historico e a fonte da verdade).
    pub fn conferir(&self) -> R<Value> {
        let mut st = self.c.prepare("SELECT i.nome,l.codigo,l.saldo,COALESCE((SELECT SUM(qtd) FROM movimentos m WHERE m.lote_id=l.id),0) FROM lotes l JOIN items i ON i.id=l.item_id")?;
        let div: Vec<Value> = st
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?)))?
            .filter_map(|x| x.ok())
            .filter(|(_, _, a, b)| a != b)
            .map(|(n, c, a, b)| json!({"item": n, "lote": c, "saldo": fl(a), "pelo_historico": fl(b)}))
            .collect();
        Ok(json!({"ok": div.is_empty(), "divergencias": div}))
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn loja() -> Estoque {
        let mut e = Estoque::memoria().ok().unwrap();
        e.hoje = tempo::parse_data("2026-11-02").unwrap();
        e
    }
    fn ok<T>(r: R<T>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => panic!("erro inesperado: {}", e.0),
        }
    }
    fn falha<T>(r: R<T>) -> String {
        match r {
            Ok(_) => panic!("deveria falhar"),
            Err(e) => e.0,
        }
    }
    fn mussarela(e: &mut Estoque) {
        ok(e.item(&json!({"nome": "Mussarela", "unidade": "kg", "unidade_compra": "peca", "fator": 3.5, "minimo": 2, "validade_dias": 20})));
    }

    #[test]
    fn fefo_sai_o_que_vence_primeiro_e_pula_vencido() {
        let mut e = loja();
        mussarela(&mut e);
        ok(e.entrada(&json!({"item": "mussarela", "quantidade": 3, "lote": "B", "validade": "2026-11-20"})));
        ok(e.entrada(&json!({"item": "mussarela", "quantidade": 2, "lote": "A", "validade": "2026-11-10"})));
        ok(e.entrada(&json!({"item": "mussarela", "quantidade": 9, "lote": "V", "validade": "2026-10-30"})));
        ok(e.saida(&json!({"item": "Mussarela", "quantidade": 3})));
        let s = ok(e.saldos());
        let lotes = &s["itens"][0]["lotes"];
        assert!(lotes.as_array().unwrap().iter().all(|l| l["lote"] != "A")); // A (vence 10/11) acabou primeiro
        assert_eq!(lotes.as_array().unwrap().iter().find(|l| l["lote"] == "B").unwrap()["saldo"], 2.0);
        assert_eq!(s["itens"][0]["utilizavel"], 2.0); // o vencido (9) nao conta
        assert!(falha(e.saida(&json!({"item": "mussarela", "quantidade": 5}))).contains("insuficiente"));
        ok(e.saida(&json!({"item": "mussarela", "quantidade": 9, "tipo": "VENCIMENTO", "lote": "V"})));
        assert_eq!(ok(e.conferir())["ok"], true);
    }

    #[test]
    fn unidade_de_compra_converte() {
        let mut e = loja();
        mussarela(&mut e);
        let r = ok(e.entrada(&json!({"item": "Mussarela", "quantidade": 2, "em_unidade_compra": true})));
        assert_eq!(r["quantidade"], 7.0);
        assert_eq!(ok(e.saldos())["itens"][0]["lotes"][0]["validade"], "2026-11-22"); // validade_dias do cadastro
        assert_eq!(norm("Coca-Cola  1,5L"), norm("coca cola 1.5l"));
    }

    #[test]
    fn historico_e_imutavel_e_estorno_uma_vez() {
        let mut e = loja();
        mussarela(&mut e);
        let m = ok(e.entrada(&json!({"item": "mussarela", "quantidade": 4})))["mov"].as_i64().unwrap();
        assert!(e.c.execute("UPDATE movimentos SET qtd=99", []).is_err());
        assert!(e.c.execute("DELETE FROM movimentos", []).is_err());
        ok(e.estorno(&json!({"mov": m})));
        assert!(falha(e.estorno(&json!({"mov": m}))).contains("ja foi estornada"));
        assert_eq!(ok(e.conferir())["ok"], true);
    }

    #[test]
    fn pedido_baixa_pela_receita_uma_unica_vez_e_marca_indisponivel() {
        let mut e = loja();
        mussarela(&mut e);
        ok(e.item(&json!({"nome": "Massa de esfiha", "unidade": "un"})));
        ok(e.receita(&json!({"produto": "Mussarela", "insumos": [{"item": "Mussarela", "quantidade": 0.1}, {"item": "Massa de esfiha", "quantidade": 1}]})));
        ok(e.entrada(&json!({"item": "mussarela", "quantidade": 0.25, "validade": "2026-11-30"})));
        ok(e.entrada(&json!({"item": "massa de esfiha", "quantidade": 50})));
        let ped = json!({"pedido": "0001", "itens": [{"nome": "Dupla Perfeita", "qtd": 1, "detalhe": "2x Mussarela"}]});
        let r = ok(e.consumir_pedido(&ped));
        assert_eq!(r["sem_receita"][0], "Dupla Perfeita"); // sem receita do combo, mas as esfihas do detalhe foram baixadas
        assert_eq!(r["baixados"].as_array().unwrap().len(), 2);
        assert_eq!(r["indisponiveis"][0]["produto"], "Mussarela"); // sobrou 0,05 kg < 0,1
        assert_eq!(ok(e.consumir_pedido(&ped))["ja_baixado"], true);
        let r2 = ok(e.consumir_pedido(&json!({"pedido": "0002", "itens": [{"nome": "Mussarela", "qtd": 1}]})));
        assert_eq!(r2["faltas"][0]["item"], "Mussarela"); // consome o que tem e avisa a falta, nunca fica negativo
        assert_eq!(ok(e.conferir())["ok"], true);
    }

    #[test]
    fn contagem_gera_ajuste() {
        let mut e = loja();
        ok(e.item(&json!({"nome": "Coca-Cola lata"})));
        ok(e.entrada(&json!({"item": "coca-cola lata", "quantidade": 16})));
        let c = ok(e.contar(&json!({"item": "Coca-Cola lata", "contado": 14})));
        assert_eq!(c["diferenca"], -2.0);
        let id = c["id"].clone();
        let r = ok(e.reconciliar(&json!({"id": id})));
        assert_eq!(r["ajuste"], -2.0);
        assert!(falha(e.reconciliar(&json!({"id": id}))).contains("ja foi reconciliada"));
        assert_eq!(ok(e.saldos())["itens"][0]["total"], 14.0);
    }

    #[test]
    fn alertas_e_compras() {
        let mut e = loja();
        mussarela(&mut e);
        ok(e.entrada(&json!({"item": "mussarela", "quantidade": 1, "validade": "2026-11-04"})));
        let a = ok(e.alertas(&json!({})));
        assert_eq!(a["abaixo_minimo"][0]["item"], "Mussarela");
        assert_eq!(a["vencendo"][0]["dias"], 2);
        let c = ok(e.compras(&json!({})));
        assert_eq!(c["sugestoes"][0]["comprar"], 1.0); // minimo 2 - saldo 1
        assert_eq!(c["sugestoes"][0]["comprar_em"]["quantidade"], 1.0);
    }

    #[test]
    fn validade_obrigatoria_so_quando_o_item_exige() {
        let mut e = loja();
        ok(e.item(&json!({"nome": "Frango", "precisa_validade": true})));
        assert!(falha(e.entrada(&json!({"item": "frango", "quantidade": 1}))).contains("precisa de validade"));
        assert!(falha(e.entrada(&json!({"item": "frango", "quantidade": 1, "validade": "2026-02-31"}))).contains("Validade invalida"));
    }
}
