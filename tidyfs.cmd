@echo off
rem 从源码目录启动交互界面：先 cargo build --release，再 npm --prefix ui run build。
node "%~dp0ui\dist\cli.js" %*
