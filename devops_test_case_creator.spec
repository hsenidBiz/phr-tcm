# -*- mode: python ; coding: utf-8 -*-
# PyInstaller one-FOLDER build. Velopack packs this folder into an installer +
# update packages via `vpk pack --packDir dist\AzureDevOpsTestCaseCreator`.
# (Velopack requires a one-folder build, not --onefile.)
import os
from PyInstaller.utils.hooks import collect_submodules

# msal pulls in submodules dynamically; collect them so the frozen build can
# complete the browser sign-in. jwt/openpyxl/velopack are safe to name explicitly.
hiddenimports = (
    collect_submodules("msal")
    + ["jwt", "openpyxl", "velopack"]
)

_icon = "resources/icon.ico" if os.path.exists("resources/icon.ico") else None
_datas = [("resources/icon.ico", "resources")] if _icon else []

a = Analysis(
    ["main.py"],
    pathex=[],
    binaries=[],
    datas=_datas,
    hiddenimports=hiddenimports,
    hookspath=[],
    runtime_hooks=[],
    excludes=[],
    noarchive=False,
)
pyz = PYZ(a.pure)

exe = EXE(
    pyz,
    a.scripts,
    [],
    exclude_binaries=True,
    name="AzureDevOpsTestCaseCreator",
    console=False,
    icon=_icon,
)
coll = COLLECT(
    exe,
    a.binaries,
    a.datas,
    name="AzureDevOpsTestCaseCreator",
)
