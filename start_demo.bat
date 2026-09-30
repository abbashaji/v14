@echo off
REM Starts a local static file server in this folder so the browser can
REM fetch essentials.afpp / the wasm file / the JS modules (this MUST be
REM served over http://localhost — opening the .html file directly with
REM file:// will not work, browsers block fetch() of local files).

cd /d "%~dp0"

where py >nul 2>nul
if %errorlevel%==0 (
  echo Starting server with "py -m http.server 8000" ...
  echo Once it says "Serving HTTP...", open this in your browser:
  echo   http://localhost:8000/packages/web/demo/crowd_dance.html
  echo.
  py -m http.server 8000
  goto :eof
)

where python >nul 2>nul
if %errorlevel%==0 (
  echo Starting server with "python -m http.server 8000" ...
  echo Once it says "Serving HTTP...", open this in your browser:
  echo   http://localhost:8000/packages/web/demo/crowd_dance.html
  echo.
  python -m http.server 8000
  goto :eof
)

where npx >nul 2>nul
if %errorlevel%==0 (
  echo No Python found. Starting server with "npx serve" instead ...
  echo Once it starts, it will print a URL - open:
  echo   [that URL]/packages/web/demo/crowd_dance.html
  echo.
  npx --yes serve -l 8000 .
  goto :eof
)

echo Could not find Python or Node/npx on this machine.
echo Install Python from https://python.org (check "Add to PATH" during
echo install) or Node from https://nodejs.org, then re-run this file.
pause
