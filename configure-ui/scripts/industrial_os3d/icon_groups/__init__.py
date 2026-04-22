from __future__ import annotations

import importlib
from types import ModuleType

GROUP_MODULE_NAMES = [
    "nav_shell",
    "dashboard_diag",
    "device_runtime",
    "tools_general",
    "tools_special",
]


def iter_group_modules(selected_groups: list[str]) -> list[ModuleType]:
    groups = selected_groups or GROUP_MODULE_NAMES
    return [
        importlib.import_module(f"industrial_os3d.icon_groups.{group_name}")
        for group_name in groups
    ]
