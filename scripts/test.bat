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
    start /b npx --yes cognito-local
    timeout /t 3 /nobreak >nul
    set TEST_COGNITO_ENDPOINT_URL=http://localhost:9229
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
