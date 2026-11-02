# Prompt para o Claude da loja instalar o ESTOQUE (cole no Claude Code, na pasta do Braniac Loja)

1. Copie o arquivo `estoque-loja.zip` para dentro da pasta do Braniac Loja (onde ficam `loja\`, `LIGAR.bat`, `cadastro.json`).
2. Cole o texto abaixo.

```
Instale o sistema de ESTOQUE no Braniac Loja desta pasta. O pacote esta em estoque-loja.zip (nesta pasta).
Regras: a Layla esta atendendo clientes, entao NAO reinicie sem eu confirmar; nao mexa em config.ini, dados\ (exceto o que o estoque cria sozinho), estado.json, chaves nem no elo P2P; nao envie nada para a internet.
Passos:
1. Extraia estoque-loja.zip aqui (cria estoque-loja\). Leia estoque-loja\aplicar.py, estoque-loja\loja\estoque_loja.py e me diga em 4 linhas o que fazem. Confirme que NADA ali usa rede (o nucleo e estoque.exe, um programa em Rust com SQLite local; o codigo-fonte esta em estoque-loja\fonte-rust\ para auditoria).
2. Antes de aplicar, me diga se loja\principal.py tem a linha exata "    srv = painel.iniciar(ctx)" dentro de main() e se painel.py tem fabricar_handler(ctx), assistente.py tem responder(texto, ctx) e principal.py tem processar_mensagens(... ao_imprimir=None). Se algo for diferente, NAO force: me mostre o trecho.
3. Rode: py -3 estoque-loja\aplicar.py "<caminho desta pasta>" --semente
   (ele guarda copias principal.py.bak_antes_estoque e cadastro.json.bak_antes_estoque, encaixa UM bloco antes de painel.iniciar, copia estoque.exe e a tela, e cadastra as bebidas com a contagem inicial.)
4. Confira sem reiniciar: py -3 -c "import loja.principal" sem erro; py -3 -m loja.estoque_loja resumo mostra "Acabando: ..."; cadastro.json ganhou "indisponiveis_manual" (igual ao que "indisponiveis" tinha antes).
5. Me mostre 3 comandas recentes da pasta comandas\ (apague nome, endereco e telefone) para eu casar os nomes dos itens do site com as receitas. Diga tambem quais nomes de combo/bebida aparecem la.
6. Quando o Carlos confirmar, feche e abra o LIGAR.bat. Depois abra http://127.0.0.1:8770/estoque (deve mostrar as bebidas) e, no chat do painel, escreva "estoque" (deve listar o que esta acabando) e "baixa 1 coca cola lata" seguido de "chegou 1 coca cola lata" (testa a baixa e a entrada).
7. Me responda em ate 8 linhas: o que mudou, o que testou, o que falhou e qualquer aviso do loja.log com "ESTOQUE".
```
