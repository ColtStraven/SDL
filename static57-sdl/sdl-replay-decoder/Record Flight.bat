@echo off
rem Record localhost:8111 telemetry on the computer that runs War Thunder.
rem Read-only. Press Enter for a mark. Type q then Enter to stop.
cd /d "%~dp0..\.."
set EXE=static57-sdl\sdl-replay-decoder\target\release\sdl-replay-decoder.exe
if not exist "%EXE%" (
  echo SDL is not built yet. Run: cargo build --release
  pause
  exit /b 1
)
"%EXE%" telemetry-record --out telemetry
pause
