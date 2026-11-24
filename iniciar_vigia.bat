@echo off
rem Inicia Vigia (version compilada en modo release) y abre el navegador.
cd /d "%~dp0"
if not exist "target\release\vigia.exe" (
  echo Compilando Vigia por primera vez. Puede tardar unos minutos...
  cargo build --release
  if errorlevel 1 (
    echo No se pudo compilar Vigia.
    pause
    exit /b 1
  )
)
rem La ventana de Vigia queda minimizada; cerrarla tambien cierra Vigia.
start "Vigia" /min "%~dp0target\release\vigia.exe"
