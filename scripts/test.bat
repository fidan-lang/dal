@echo off
rem Run the full Dal integration test suite on Windows.
rem Usage: scripts\test.bat [--with-cognito]

setlocal enabledelayedexpansion
cd /d "%~dp0.."

echo ==^> Starting test infrastructure (docker-compose.test.yml)...
docker compose -f docker-compose.test.yml up -d --wait --quiet-pull
if errorlevel 1 (
    echo FAILED to start test infrastructure.
    exit /b 1
)

if not exist .env.test (
    copy .env.test.example .env.test >nul
    echo ==^> Created .env.test from .env.test.example
)

set EXTRA_ARGS=
if "%1"=="--with-cognito" (
    echo ==^> Starting cognito-local...
    if not exist LOCAL mkdir LOCAL
    set PORT=9229
    start /b cmd /c "npx --yes cognito-local > LOCAL\\cognito-local.log 2>&1"
    set TEST_COGNITO_ENDPOINT_URL=http://127.0.0.1:9229
    set TEST_COGNITO_POOL_ID=local_0dPm2L0N
    set TEST_COGNITO_CLIENT_ID=bs5obcfdxmvh7g6ldnqmeahae
    powershell -NoProfile -Command "$ErrorActionPreference='Stop'; for ($i = 0; $i -lt 20; $i++) { try { Invoke-WebRequest -UseBasicParsing 'http://127.0.0.1:9229/local_0dPm2L0N/.well-known/jwks.json' | Out-Null; exit 0 } catch { Start-Sleep -Seconds 1 } }; exit 1"
    if errorlevel 1 (
        echo FAILED to start cognito-local. See LOCAL\cognito-local.log
        if exist LOCAL\cognito-local.log type LOCAL\cognito-local.log
        exit /b 1
    )
    set EXTRA_ARGS=-- --include-ignored
)

echo ==^> Running integration tests...
cargo test -p dal-server %EXTRA_ARGS% -- --test-threads=1
set TEST_EXIT=%errorlevel%

echo ==^> Stopping test infrastructure...
docker compose -f docker-compose.test.yml down

if %TEST_EXIT% neq 0 (
    echo FAILED — some tests did not pass.
    exit /b %TEST_EXIT%
)
echo ==^> All tests passed.
