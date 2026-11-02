# -*- coding: utf-8 -*-
r"""Instala o ESTOQUE no Braniac Loja JA EM USO. Copia estoque\estoque.exe, loja\estoque_loja.py e loja\web\estoque.html e acrescenta UM bloco
em loja\principal.py (antes de 'srv = painel.iniciar(ctx)'). Nao troca nenhum outro arquivo da Layla. Faz copias (.bak_antes_estoque)
e e seguro rodar de novo. Com --semente cadastra as bebidas e a contagem inicial (so se o estoque ainda estiver vazio)."""
import shutil
import subprocess
import sys
from pathlib import Path

AQUI = Path(__file__).resolve().parent
args = [a for a in sys.argv[1:] if not a.startswith("--")]
RAIZ = Path(args[0]) if args else AQUI.parent
ANCORA = "    srv = painel.iniciar(ctx)\n"
BLOCO = '''    try:    # estoque (estoque.exe em Rust): baixa por pedido, tela /estoque, comandos no chat; se algo faltar, so o estoque fica de fora
        from . import estoque_loja
        estoque_loja.instalar(ctx)
    except Exception as e:
        log(f"estoque nao iniciou (rode ATUALIZAR-ESTOQUE.bat): {e!r}")
'''


def main():
    pr = RAIZ / "loja" / "principal.py"
    if not pr.exists():
        return print("Nao achei loja/principal.py em", RAIZ, "- rode: py -3 estoque-loja\aplicar.py CAMINHO_DA_PASTA")
    (RAIZ / "estoque").mkdir(exist_ok=True)
    (RAIZ / "loja" / "web").mkdir(exist_ok=True)
    shutil.copy2(AQUI / "estoque" / "estoque.exe", RAIZ / "estoque" / "estoque.exe")
    shutil.copy2(AQUI / "loja" / "estoque_loja.py", RAIZ / "loja" / "estoque_loja.py")
    shutil.copy2(AQUI / "loja" / "web" / "estoque.html", RAIZ / "loja" / "web" / "estoque.html")
    cad = RAIZ / "cadastro.json"
    if cad.exists() and not (RAIZ / "cadastro.json.bak_antes_estoque").exists():
        shutil.copy2(cad, RAIZ / "cadastro.json.bak_antes_estoque")
    s = pr.read_text(encoding="utf-8")
    if "estoque_loja" in s:
        print("principal.py ja tem o estoque. Arquivos atualizados.")
    elif s.count(ANCORA) != 1:
        return print("Nao achei o ponto de encaixe em principal.py (versao diferente). Nada foi alterado em principal.py; mande este aviso ao Carlos e mostre a funcao main().")
    else:
        shutil.copy2(pr, pr.with_name("principal.py.bak_antes_estoque"))
        novo = s.replace(ANCORA, BLOCO + ANCORA, 1)
        compile(novo, str(pr), "exec")
        pr.write_text(novo, encoding="utf-8")
        print("Estoque encaixado em principal.py.")
    py = sys.executable
    v = subprocess.run([py, "-c", "import json;from loja import estoque_loja as E;print(json.dumps(E.rodar('versao')))"], cwd=RAIZ, capture_output=True, text=True)
    print("Teste do estoque.exe:", (v.stdout or v.stderr).strip())
    if "--semente" in sys.argv:
        print(subprocess.run([py, "-m", "loja.estoque_loja", "semente"], cwd=RAIZ, capture_output=True, text=True, encoding="utf-8").stdout.strip())
    print("Pronto. Feche e abra o LIGAR.bat (quando a loja puder ficar 1 minuto parada). A tela fica em http://127.0.0.1:8770/estoque")


main()
