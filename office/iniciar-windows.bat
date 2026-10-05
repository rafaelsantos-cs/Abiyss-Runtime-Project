@echo off
setlocal
chcp 65001 >nul
cd /d "%~dp0"
title Abiyss Office

where node >nul 2>nul
if errorlevel 1 (
  echo.
  echo   O Node.js nao foi encontrado neste PC.
  echo   Instale a versao LTS em https://nodejs.org/ ^(22.13 ou mais nova^)
  echo   e depois clique de novo em iniciar-windows.bat.
  echo.
  start "" https://nodejs.org/
  pause
  exit /b 1
)

node -e "const [a,b]=process.versions.node.split('.').map(Number);process.exit(a>22||(a===22&&b>=13)?0:1)"
if errorlevel 1 (
  echo.
  echo   Seu Node.js e antigo demais para o Abiyss Office ^(precisa da 22.13 ou mais nova^).
  echo   Atualize em https://nodejs.org/ e tente de novo.
  echo.
  pause
  exit /b 1
)

if not exist "node_modules\three\build\three.module.js" (
  echo   Instalando a dependencia ^(three.js^)...
  call npm ci --omit=dev
  if errorlevel 1 (
    pause
    exit /b 1
  )
)

echo.
echo   Abiyss Office iniciando em http://127.0.0.1:8090/
echo   O navegador abre sozinho. Para parar: feche esta janela ou aperte Ctrl+C.
echo.
node --disable-warning=ExperimentalWarning server\main.js --open %*
pause
