@echo off
setlocal EnableExtensions DisableDelayedExpansion
pushd "%~dp0.." || exit /b 1
set "ROOT=%CD%"
set "ISCC="
set "TOOLCHAIN="
set "OFFLINE="
set "OUTPUT_DIR=%ROOT%\dist"
set "STAGING="
set "POMELO_PACKAGE_ICON="
set "APP_VERSION="
set "TARGET_DIR=%ROOT%\target"
if defined CARGO_TARGET_DIR for %%I in ("%CARGO_TARGET_DIR%") do set "TARGET_DIR=%%~fI"

:arguments
if "%~1"=="" goto prepare
if /i "%~1"=="--help" goto help
if /i "%~1"=="-h" goto help
if /i "%~1"=="--offline" goto offline
if /i "%~1"=="--iscc" goto iscc_argument
if /i "%~1"=="--toolchain" goto toolchain_argument
if /i "%~1"=="--output-dir" goto output_argument
echo Unknown option: %~1 1>&2
goto failed

:offline
set "OFFLINE=--offline"
shift
goto arguments

:iscc_argument
if "%~2"=="" goto missing_value
set "ISCC=%~f2"
shift
shift
goto arguments

:toolchain_argument
if "%~2"=="" goto missing_value
set "TOOLCHAIN=+%~2"
shift
shift
goto arguments

:output_argument
if "%~2"=="" goto missing_value
set "OUTPUT_DIR=%~f2"
shift
shift
goto arguments

:missing_value
echo Missing value for %~1. 1>&2
goto failed

:prepare
where cargo.exe >nul 2>&1
if errorlevel 1 (
    echo Cargo was not found in PATH. Install Rust and the MSVC build tools. 1>&2
    goto failed
)
if defined ISCC goto check_iscc
for /f "delims=" %%I in ('where ISCC.exe 2^>nul') do if not defined ISCC set "ISCC=%%I"
if not defined ISCC if exist "%ProgramFiles(x86)%\Inno Setup 6\ISCC.exe" set "ISCC=%ProgramFiles(x86)%\Inno Setup 6\ISCC.exe"
if not defined ISCC if exist "%LOCALAPPDATA%\Programs\Inno Setup 6\ISCC.exe" set "ISCC=%LOCALAPPDATA%\Programs\Inno Setup 6\ISCC.exe"

:check_iscc
if not defined ISCC goto missing_iscc
if not exist "%ISCC%" goto missing_iscc
rem The workspace version is the first top-level version assignment in Cargo.toml.
for /f "tokens=2 delims== " %%V in ('findstr /r /c:"^version[ ]*=[ ]*" Cargo.toml') do if not defined APP_VERSION set "APP_VERSION=%%~V"
if not defined APP_VERSION (
    echo Could not read the workspace version from Cargo.toml. 1>&2
    goto failed
)
if not exist "%OUTPUT_DIR%\." mkdir "%OUTPUT_DIR%"
if not exist "%OUTPUT_DIR%\." (
    echo Could not create the installer output directory. 1>&2
    goto failed
)

:create_staging
set "STAGING=%TEMP%\Pomelo-package-%RANDOM%-%RANDOM%"
if exist "%STAGING%" goto create_staging
mkdir "%STAGING%"
if errorlevel 1 goto failed
rem A unique path makes Cargo regenerate and export the current SVG icon.
set "POMELO_PACKAGE_ICON=%STAGING%\pomelo.ico"
cargo %TOOLCHAIN% build -p pomelo --release --locked --target x86_64-pc-windows-msvc --target-dir "%TARGET_DIR%" %OFFLINE%
if errorlevel 1 goto failed
set "BUILD_DIR=%TARGET_DIR%\x86_64-pc-windows-msvc\release"
if not exist "%BUILD_DIR%\pomelo.exe" (
    echo Cargo did not produce pomelo.exe. 1>&2
    goto failed
)
if not exist "%POMELO_PACKAGE_ICON%" (
    echo Cargo did not export the installer icon. 1>&2
    goto failed
)
"%ISCC%" "/DAppVersion=%APP_VERSION%" "/DBuildDir=%BUILD_DIR%" "/DIconFile=%POMELO_PACKAGE_ICON%" "/DOutputDir=%OUTPUT_DIR%" "%ROOT%\scripts\windows\pomelo.iss"
if errorlevel 1 goto failed
echo Installer: "%OUTPUT_DIR%\Pomelo-%APP_VERSION%-windows-x64-setup.exe"
set "EXIT_CODE=0"
goto cleanup

:missing_iscc
echo Install Inno Setup 6 or pass --iscc with the path to ISCC.exe. 1>&2
goto failed

:help
echo Usage: scripts\build-windows.bat [--iscc PATH] [--toolchain NAME] [--offline] [--output-dir PATH]
echo Requires Rust, MSVC build tools, the Windows SDK, and Inno Setup 6.
echo The installer is written to dist by default. CARGO_TARGET_DIR is supported.
set "EXIT_CODE=0"
goto cleanup

:failed
set "EXIT_CODE=1"

:cleanup
if defined STAGING if exist "%STAGING%" rmdir /s /q "%STAGING%"
popd
endlocal & exit /b %EXIT_CODE%
