# -*- coding: utf-8 -*-
"""Estoque do Braniac Loja. O nucleo e o estoque.exe (Rust + SQLite): lotes com validade, saida FEFO, historico imutavel, receitas, alertas.
Este arquivo e so a cola: chama o .exe, da baixa a cada pedido impresso, tira da Layla o que acabou, serve a tela /estoque e responde comandos no chat.

Nada aqui apaga ou muda arquivos da Layla: instalar() so "veste" funcoes ja existentes (e desfaz sozinho se algo nao encaixar)."""
import inspect
import json
import os
import re
import subprocess
import sys
import threading
import time
import unicodedata
from datetime import datetime
from pathlib import Path

RAIZ = Path(__file__).resolve().parent.parent
EXE = Path(os.environ.get("ESTOQUE_EXE") or RAIZ / "estoque" / "estoque.exe")
DADOS = RAIZ / "dados"
DB = Path(os.environ.get("ESTOQUE_DB") or DADOS / "estoque.db")
_trava = threading.Lock()
_instalado = False

LEITURA = {"saldos", "alertas", "compras", "movimentos", "indisponiveis", "conferir"}
ESCRITA = {"item", "receita", "entrada", "saida", "ajuste", "estorno", "contar", "reconciliar"}


def _log(msg):
    try:
        from . import principal
        principal.log("ESTOQUE " + msg)
    except Exception:
        print("ESTOQUE", msg, file=sys.stderr)


def rodar(cmd, dados=None, timeout=30):
    """Chama o estoque.exe. Sempre devolve um dict com 'ok'."""
    if not EXE.exists():
        return {"ok": False, "erro": f"estoque.exe nao encontrado em {EXE} (rode ATUALIZAR-ESTOQUE.bat)"}
    DB.parent.mkdir(parents=True, exist_ok=True)
    try:
        with _trava:
            p = subprocess.run([str(EXE), "--db", str(DB), cmd], input=json.dumps(dados or {}).encode("utf-8"), capture_output=True,
                               timeout=timeout, creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        return json.loads(p.stdout.decode("utf-8"))
    except subprocess.TimeoutExpired:
        return {"ok": False, "erro": "o estoque demorou demais"}
    except (OSError, ValueError) as e:
        return {"ok": False, "erro": f"falha ao chamar o estoque: {e}"}


# ---------- Layla: o que acabou sai do cardapio ----------
def sincronizar_indisponiveis():
    """Une a lista MANUAL de sabores fora (cadastro.json 'indisponiveis_manual') com o que o estoque diz que acabou, e grava em 'indisponiveis'
    (a Layla le essa lista a cada ciclo). Na 1a vez, o que ja estava em 'indisponiveis' vira a lista manual."""
    r = rodar("indisponiveis")
    if not r.get("ok"):
        return
    auto = sorted({x["produto"] for x in r.get("indisponiveis", [])})
    cad_arq, est_arq = RAIZ / "cadastro.json", DADOS / "estoque_auto.json"
    try:
        cad = json.loads(cad_arq.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return
    try:
        antes = json.loads(est_arq.read_text(encoding="utf-8")).get("auto", [])
    except (OSError, ValueError):
        antes = []
    manual = cad.get("indisponiveis_manual")
    if manual is None:
        manual = [x for x in cad.get("indisponiveis", []) if x not in antes]
    novo = list(manual) + [a for a in auto if a not in manual]
    if cad.get("indisponiveis") == novo and cad.get("indisponiveis_manual") == manual:
        return
    cad["indisponiveis_manual"], cad["indisponiveis"] = manual, novo
    tmp = cad_arq.with_suffix(".json.tmp")
    tmp.write_text(json.dumps(cad, ensure_ascii=False, indent=2), encoding="utf-8")
    os.replace(tmp, cad_arq)
    DADOS.mkdir(exist_ok=True)
    est_arq.write_text(json.dumps({"auto": auto}, ensure_ascii=False), encoding="utf-8")
    if set(auto) != set(antes):
        _log(f"indisponiveis pelo estoque: {', '.join(auto) or 'nenhum'}")


def baixar_pedido(ped, numero, quando):
    """Chamado depois que a comanda sai na impressora. Idempotente por pedido."""
    if not ped.get("itens"):
        return
    r = rodar("pedido", {"pedido": f"{quando:%Y%m%d}-{numero:04d}",
                         "itens": [{"nome": i["nome"], "qtd": i["qtd"], "detalhe": i.get("detalhe", "")} for i in ped["itens"]]})
    if not r.get("ok"):
        return _log(f"pedido #{numero:04d} sem baixa: {r.get('erro')}")
    if r.get("faltas"):
        _log(f"pedido #{numero:04d}: FALTOU no estoque: " + ", ".join(f"{f['item']} ({f['faltou']} {f['unidade']})" for f in r["faltas"]))
    if r.get("sem_receita"):
        _log(f"pedido #{numero:04d}: sem receita (nao baixou): " + ", ".join(r["sem_receita"]))
    sincronizar_indisponiveis()


# ---------- comandos no chat do painel ----------
def _n(t):
    return "".join(c for c in unicodedata.normalize("NFD", (t or "").lower()) if unicodedata.category(c) != "Mn").replace("-", " ").replace(",", ".")


def _achar(nome):
    """(nome_exato, None) ou (None, mensagem de erro). Aceita parte do nome se for unica."""
    itens = [i["item"] for i in rodar("saldos").get("itens", [])]
    alvo = " ".join(_n(nome).split())
    exato = [i for i in itens if " ".join(_n(i).split()) == alvo]
    if exato:
        return exato[0], None
    parte = [i for i in itens if all(p in _n(i) for p in alvo.split())]
    if len(parte) == 1:
        return parte[0], None
    if parte:
        return None, "Qual deles? " + "; ".join(parte[:8])
    return None, f"Nao achei '{nome}' no estoque. Escreva 'estoque' para ver a lista."


def resumo():
    a = rodar("alertas")
    if not a.get("ok"):
        return "Estoque indisponivel: " + a.get("erro", "")
    L = []
    if a["indisponiveis"]:
        L.append("🚫 Fora do cardapio por falta: " + ", ".join(x["produto"] for x in a["indisponiveis"]))
    if a["abaixo_minimo"]:
        L.append("⚠️ Acabando: " + ", ".join(f"{x['item']} ({x['utilizavel']} {x['unidade']})" for x in a["abaixo_minimo"]))
    for v in a["vencendo"][:8]:
        L.append(("❌ VENCIDO: " if v["vencido"] else f"⏳ Vence em {v['dias']} dia(s): ") + f"{v['item']} lote {v['lote']} ({v['saldo']} {v['unidade']})")
    return "\n".join(L) or "Estoque em dia: nada acabando e nada vencendo. ✅"


def lista_compras():
    c = rodar("compras")
    if not c.get("ok"):
        return "Compras indisponivel: " + c.get("erro", "")
    if not c["sugestoes"]:
        return "Nada para comprar agora. ✅"
    out = []
    for s in c["sugestoes"]:
        q = s.get("comprar_em")
        dias = "" if s["dias_restantes"] is None else f", dura ~{s['dias_restantes']} dia(s)"
        out.append(f"• {s['item']}: comprar {q['quantidade']:g} {q['unidade']}" if q else f"• {s['item']}: comprar {s['comprar']:g} {s['unidade']}")
        out[-1] += f" [{s['urgencia']}]{dias}"
    return "Lista de compras (cobre " + f"{c['dias_cobertura']:g} dias):\n" + "\n".join(out)


def responder_comando(texto):
    """Devolve a resposta se o texto for um comando de estoque, senao None."""
    t = " ".join((texto or "").strip().split())
    n = _n(t)
    if n in ("estoque", "alertas", "o que ta acabando", "o que esta acabando", "o que falta"):
        return resumo()
    if n in ("compras", "lista de compras", "o que comprar"):
        return lista_compras()
    m = re.match(r"^(entrada|chegou|comprei|baixa|saida|perda|vencido|contagem|contei)\s+([\d.,]+)\s+(?:de\s+)?(.+)$", t, re.I)
    if not m:
        return None
    verbo, qtd, nome = m.group(1).lower(), float(m.group(2).replace(",", ".")), m.group(3)
    item, erro = _achar(nome)
    if erro:
        return erro
    if verbo in ("entrada", "chegou", "comprei"):
        r = rodar("entrada", {"item": item, "quantidade": qtd, "motivo": "Pelo chat"})
        if not r.get("ok") and "precisa de validade" in r.get("erro", ""):
            return r["erro"] + ". Use a tela de estoque para informar a validade."
    elif verbo in ("contagem", "contei"):
        c = rodar("contar", {"item": item, "contado": qtd})
        if not c.get("ok"):
            return c.get("erro", "Nao consegui contar.")
        r = rodar("reconciliar", {"id": c["id"]})
        if r.get("ok"):
            sincronizar_indisponiveis()
            return f"Contagem de {item}: sistema tinha {c['sistema']}, voce contou {c['contado']} (ajuste {r['ajuste']:+g})."
    else:
        tipo = {"perda": "PERDA", "vencido": "VENCIMENTO"}.get(verbo, "CONSUMO")
        r = rodar("saida", {"item": item, "quantidade": qtd, "tipo": tipo, "motivo": "Pelo chat"})
    if not r.get("ok"):
        return r.get("erro", "Nao consegui registrar.")
    sincronizar_indisponiveis()
    s = next((i for i in rodar("saldos").get("itens", []) if i["item"] == item), None)
    return f"Registrado: {verbo} de {qtd:g} {item}." + (f" Agora tem {s['utilizavel']:g} {s['unidade']}." if s else "")


# ---------- instalacao (veste funcoes ja existentes) ----------
def _pagina():
    return (Path(__file__).resolve().parent / "web" / "estoque.html").read_bytes()


def _vestir_painel(painel):
    orig = painel.fabricar_handler
    LINK = ('<a href="/estoque" style="position:fixed;right:14px;bottom:14px;z-index:9;background:#c4161c;color:#fff;padding:10px 16px;'
            'border-radius:99px;font:600 14px system-ui;text-decoration:none;box-shadow:0 2px 8px #0004">📦 Estoque</a>').encode("utf-8")

    def fabricar(ctx):
        Base = orig(ctx)

        class H(Base):
            def _bytes(self, corpo, tipo="text/html; charset=utf-8"):
                self.send_response(200)
                self.send_header("Content-Type", tipo)
                self.send_header("Content-Length", str(len(corpo)))
                self.send_header("Cache-Control", "no-store")
                self.end_headers()
                self.wfile.write(corpo)

            def do_GET(self):
                caminho, _, consulta = self.path.partition("?")
                if caminho == "/estoque" and self._origem_ok():
                    return self._bytes(_pagina())
                if caminho.startswith("/api/estoque/") and self._origem_ok():
                    cmd = caminho.rsplit("/", 1)[-1]
                    if cmd not in LEITURA:
                        return self._json({"ok": False, "erro": "comando desconhecido"}, 404)
                    dados = {k: v for k, _, v in (p.partition("=") for p in consulta.split("&") if p)}
                    return self._json(rodar(cmd, dados))
                if caminho in ("/", "/index.html") and self._origem_ok() and hasattr(painel, "PAGINA"):
                    return self._bytes(painel.PAGINA.read_bytes().replace(b"</body>", LINK + b"</body>", 1))
                return super().do_GET()

            def do_POST(self):
                if self.path.startswith("/api/estoque/"):
                    try:
                        n = int(self.headers.get("Content-Length") or 0)
                        bruto = self.rfile.read(min(n, 20000))
                    except (ValueError, OSError):
                        return self._json({"ok": False, "erro": "pedido invalido"}, 400)
                    if not self._origem_ok() or "json" not in (self.headers.get("Content-Type") or ""):
                        return self._json({"ok": False, "erro": "acesso negado"}, 403)
                    cmd = self.path.rsplit("/", 1)[-1]
                    if cmd not in ESCRITA and cmd not in LEITURA:
                        return self._json({"ok": False, "erro": "comando desconhecido"}, 404)
                    try:
                        dados = json.loads(bruto or b"{}")
                    except ValueError:
                        return self._json({"ok": False, "erro": "pedido invalido"}, 400)
                    r = rodar(cmd, dados)
                    if r.get("ok") and cmd in ESCRITA:
                        sincronizar_indisponiveis()
                    return self._json(r)
                return super().do_POST()

        return H

    painel.fabricar_handler = fabricar


def _vestir_baixa(principal):
    orig = principal.processar_mensagens
    sig = inspect.signature(orig)
    if "ao_imprimir" not in sig.parameters:
        return _log("processar_mensagens desta versao nao tem 'ao_imprimir': baixa automatica NAO instalada")

    def novo(*a, **k):
        ba = sig.bind(*a, **k)
        ba.apply_defaults()
        depois = ba.arguments.get("ao_imprimir")

        def junto(ped, numero, quando):
            try:
                baixar_pedido(ped, numero, quando)
            except Exception as e:
                _log(f"erro na baixa do pedido #{numero}: {e!r}")
            if depois:
                depois(ped, numero, quando)

        ba.arguments["ao_imprimir"] = junto
        return orig(*ba.args, **ba.kwargs)

    principal.processar_mensagens = novo


def _vestir_chat(assistente):
    orig = assistente.responder

    def responder(texto, ctx, *a, **k):
        try:
            r = responder_comando(texto)
        except Exception as e:
            r = f"Tive um problema no estoque ({str(e)[:80]})."
        return r if r is not None else orig(texto, ctx, *a, **k)

    assistente.responder = responder


def _rotina():
    """Backup diario do banco (guarda os ultimos 14) e conferencia saldo x historico."""
    while True:
        try:
            pasta = DADOS / "backups"
            pasta.mkdir(parents=True, exist_ok=True)
            alvo = pasta / f"estoque-{datetime.now():%Y%m%d}.db"
            if not alvo.exists() and DB.exists():
                rodar("backup", {"para": str(alvo)})
                for velho in sorted(pasta.glob("estoque-*.db"))[:-14]:
                    velho.unlink(missing_ok=True)
            c = rodar("conferir")
            if c.get("ok") and c.get("divergencias"):
                _log("DIVERGENCIA entre saldo e historico: " + json.dumps(c["divergencias"], ensure_ascii=False)[:300])
            sincronizar_indisponiveis()   # vencimento muda a lista sem ninguem mexer
        except Exception as e:
            _log(f"rotina: {e!r}")
        time.sleep(3600)


def instalar(ctx=None):
    """Chamada uma vez, ANTES de painel.iniciar(ctx). Se alguma peca nao encaixar, so ela fica de fora e o resto da loja segue igual."""
    global _instalado
    if _instalado:
        return
    _instalado = True
    if not EXE.exists():
        return _log(f"estoque.exe nao encontrado em {EXE}: estoque desligado")
    from . import assistente, painel, principal
    for nome, fn, alvo in (("painel", _vestir_painel, painel), ("baixa de pedidos", _vestir_baixa, principal), ("chat", _vestir_chat, assistente)):
        try:
            fn(alvo)
        except Exception as e:
            _log(f"{nome} nao instalado: {e!r}")
    threading.Thread(target=_rotina, daemon=True, name="estoque").start()
    _log("estoque ligado (tela em /estoque)")


# ---------- estoque inicial (contagem do dono em 02/11/2026) ----------
BEBIDAS = [  # (nome, quantidade, minimo sugerido)  ← minimos sao palpite: ajuste na tela
    ("Coca-Cola lata", 16, 6), ("Coca-Cola Zero lata", 2, 0), ("Guaraná lata", 17, 6), ("Guaraná Zero lata", 1, 0),
    ("Coca-Cola 1L", 7, 2), ("Guaraná 1L", 1, 2),
    ("Coca-Cola 1,5L", 2, 2),
    ("Coca-Cola 2L", 2, 1), ("Coca-Cola Zero 2L", 1, 0), ("Guaraná 2L", 2, 1), ("Guaraná Zero 2L", 1, 0),
]
COMBOS = {"Dupla Perfeita": "Guaraná lata", "Quarteto Imperdível": "Guaraná 1L", "Combo Galera": "Coca-Cola 1L", "Festival de Esfihas": "Coca-Cola 1,5L"}


def semente():
    """Cadastra as bebidas, as receitas (cada bebida vende a si mesma; combos baixam a bebida deles) e a contagem inicial. Roda uma vez so."""
    if rodar("movimentos", {"limite": 1}).get("movimentos"):
        return print("Estoque ja tem movimentos: semente ignorada.")
    for nome, q, minimo in BEBIDAS:
        for r in (rodar("item", {"nome": nome, "unidade": "un", "minimo": minimo}),
                  rodar("receita", {"produto": nome, "insumos": [{"item": nome, "quantidade": 1}]}),
                  rodar("entrada", {"item": nome, "quantidade": q, "motivo": "Contagem inicial 02/11/2026"})):
            if not r.get("ok"):
                print("ERRO:", nome, r.get("erro"))
    for combo, bebida in COMBOS.items():
        rodar("receita", {"produto": combo, "insumos": [{"item": bebida, "quantidade": 1}]})
    sincronizar_indisponiveis()
    print("Estoque inicial de bebidas gravado.")


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8")
    comando = sys.argv[1] if len(sys.argv) > 1 else ""
    if comando == "semente":
        semente()
    elif comando == "resumo":
        print(resumo())
    else:
        print("uso: python -m loja.estoque_loja semente|resumo")
