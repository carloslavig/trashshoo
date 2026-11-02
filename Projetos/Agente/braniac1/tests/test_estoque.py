# -*- coding: utf-8 -*-
"""Cola Python do estoque (loja/estoque_loja.py) contra o estoque.exe de verdade. Pula se o .exe nao estiver compilado."""
import json
import sys
import urllib.request
from datetime import datetime
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import pytest
from loja import estoque_loja as E, painel, principal

pytestmark = pytest.mark.skipif(not E.EXE.exists(), reason="estoque.exe nao compilado")


@pytest.fixture(autouse=True)
def _loja(tmp_path, monkeypatch):
    monkeypatch.setattr(E, "DB", tmp_path / "dados" / "estoque.db")
    monkeypatch.setattr(E, "DADOS", tmp_path / "dados")
    monkeypatch.setattr(E, "RAIZ", tmp_path)
    (tmp_path / "cadastro.json").write_text(json.dumps({"loja": "L", "indisponiveis": ["Brócolis Especial"]}, ensure_ascii=False), encoding="utf-8")
    return tmp_path


def cad(tmp):
    return json.loads((tmp / "cadastro.json").read_text(encoding="utf-8"))


def test_semente_e_pedido_tiram_da_layla_o_que_acabou(_loja):
    E.semente()
    s = {i["item"]: i for i in E.rodar("saldos")["itens"]}
    assert s["Coca-Cola lata"]["total"] == 16 and s["Guaraná 1L"]["total"] == 1
    assert "Combo Galera" not in cad(_loja)["indisponiveis"]
    ped = {"itens": [{"qtd": 1, "nome": "Combo Galera", "detalhe": "6 esfihas", "preco": ""}, {"qtd": 1, "nome": "Guaraná 1L", "detalhe": "", "preco": ""}]}
    E.baixar_pedido(ped, 1, datetime(2026, 11, 2, 20, 0))
    c = cad(_loja)
    assert "Guaraná 1L" in c["indisponiveis"]                         # o ultimo acabou: a Layla para de vender
    assert "Brócolis Especial" in c["indisponiveis"]                  # a lista manual continua intacta
    assert c["indisponiveis_manual"] == ["Brócolis Especial"]
    E.baixar_pedido(ped, 1, datetime(2026, 11, 2, 20, 5))             # mesmo pedido de novo: nao baixa duas vezes
    assert {i["item"]: i for i in E.rodar("saldos")["itens"]}["Coca-Cola 1L"]["total"] == 6


def test_entrada_devolve_o_item_ao_cardapio(_loja):
    E.semente()
    E.baixar_pedido({"itens": [{"qtd": 1, "nome": "Guaraná 1L", "detalhe": ""}]}, 7, datetime(2026, 11, 2, 21, 0))
    assert "Guaraná 1L" in cad(_loja)["indisponiveis"]
    assert "Registrado" in E.responder_comando("chegou 6 guarana 1l")
    assert "Guaraná 1L" not in cad(_loja)["indisponiveis"]


def test_comandos_do_chat(_loja):
    E.semente()
    assert "Acabando" in E.responder_comando("estoque")
    assert "Qual deles" in E.responder_comando("baixa 1 coca")
    assert "Nao achei" in E.responder_comando("baixa 1 tapioca")
    assert "Agora tem 15" in E.responder_comando("baixa 1 coca cola lata")
    assert "ajuste -1" in E.responder_comando("contei 14 coca cola lata")
    assert E.responder_comando("oi tudo bem") is None
    assert "Estoque insuficiente" in E.responder_comando("baixa 99 coca cola lata")


def test_painel_serve_a_tela_e_a_api_sem_quebrar_o_resto(_loja, monkeypatch):
    monkeypatch.setattr(E, "_instalado", False)
    monkeypatch.setattr(painel, "fabricar_handler", painel.fabricar_handler)
    monkeypatch.setattr(principal, "processar_mensagens", principal.processar_mensagens)
    from loja import assistente
    monkeypatch.setattr(assistente, "responder", assistente.responder)
    E.semente()
    E._vestir_painel(painel)
    srv = painel.iniciar({"cfg": {"loja": "L", "responder": True, "feedback": True, "chaves": {}}, "estado": principal.Estado(_loja / "e.json"),
                          "vivo": {"logado": True, "erro": ""}}, porta=0)
    try:
        base = f"http://127.0.0.1:{srv.server_address[1]}"
        pag = urllib.request.urlopen(base + "/estoque").read().decode("utf-8")
        assert "Estoque" in pag and "/api/estoque/" in pag
        assert json.loads(urllib.request.urlopen(base + "/api/estoque/saldos").read())["ok"]
        assert json.loads(urllib.request.urlopen(base + "/api/estoque/movimentos?limite=3").read())["movimentos"]
        req = urllib.request.Request(base + "/api/estoque/entrada", json.dumps({"item": "Coca-Cola 2L", "quantidade": 3}).encode(), {"Content-Type": "application/json"})
        assert json.loads(urllib.request.urlopen(req).read())["ok"]
        ruim = urllib.request.Request(base + "/api/estoque/backup", json.dumps({"para": "x"}).encode(), {"Content-Type": "application/json"})
        with pytest.raises(urllib.error.HTTPError):
            urllib.request.urlopen(ruim)                              # backup com caminho livre nunca fica exposto
        assert b"Estoque" in urllib.request.urlopen(base + "/").read()   # o painel antigo continua la, com o botao novo
        assert json.loads(urllib.request.urlopen(base + "/api/status").read())["responder"] is True
    finally:
        srv.shutdown()


def test_baixa_encadeia_com_o_que_ja_existia(_loja, monkeypatch):
    E.semente()
    visto = []
    chamadas = []
    monkeypatch.setattr(principal, "processar_mensagens", lambda msgs, contato, estado, cfg, imprimir=None, pasta=None, ao_imprimir=None: ao_imprimir({"itens": []}, 3, datetime.now()) or 1)
    E._vestir_baixa(principal)
    monkeypatch.setattr(E, "baixar_pedido", lambda *a: chamadas.append(a))
    assert principal.processar_mensagens([], "c", None, {}, ao_imprimir=lambda *a: visto.append(a)) == 1
    assert len(chamadas) == 1 and len(visto) == 1                    # as duas coisas rodaram
