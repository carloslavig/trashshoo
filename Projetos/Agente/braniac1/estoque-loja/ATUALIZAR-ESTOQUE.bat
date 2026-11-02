@echo off
rem Instala o estoque no Braniac Loja. Copie a pasta estoque-loja para dentro da pasta do Braniac Loja e de duplo clique aqui.
cd /d "%~dp0.."
call _py.bat
%PY% estoque-loja\aplicar.py "%CD%" --semente
pause
