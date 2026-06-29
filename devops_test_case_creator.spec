# -*- mode: python ; coding: utf-8 -*-
# PyInstaller one-FOLDER build. Velopack packs this folder into an installer +
# update packages via `vpk pack --packDir dist\AzureDevOpsTestCaseCreator`.
# (Velopack requires a one-folder build, not --onefile.)
import os
from PyInstaller.utils.hooks import collect_submodules, collect_all

# pymsalruntime is the native Windows WAM broker runtime (one-click sign-in);
# collect its hidden submodules + native binaries + data files. VERIFY in the
# frozen build that the broker sign-in works — if the runtime isn't bundled MSAL
# falls back to the system browser, so sign-in still works either way.
_pymsal_datas, _pymsal_bins, _pymsal_hidden = collect_all("pymsalruntime")

# msal pulls in submodules dynamically; collect them so the frozen build can
# complete sign-in. jwt/openpyxl/velopack are safe to name explicitly.
hiddenimports = (
    collect_submodules("msal")
    + ["jwt", "openpyxl", "velopack", "PyQt5.QtSvg"]
    + _pymsal_hidden
)

_icon = "resources/icon.ico" if os.path.exists("resources/icon.ico") else None
_datas = [("resources/icon.ico", "resources")] if _icon else []
# Bundle the themed SVG icon set (rendered at runtime via QtSvg).
if os.path.isdir("resources/icons"):
    _datas += [("resources/icons", "resources/icons")]

a = Analysis(
    ["main.py"],
    pathex=[],
    binaries=_pymsal_bins,
    datas=_datas + _pymsal_datas,
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
