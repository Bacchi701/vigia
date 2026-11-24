@echo off
rem Cierra Vigia de forma ordenada (guarda el ultimo minuto del historial).
cd /d "%~dp0"
if not exist "target\release\vigia.exe" (
  echo Vigia no esta compilado.
  exit /b 0
)
"%~dp0target\release\vigia.exe" --detener
