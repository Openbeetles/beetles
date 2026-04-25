from __future__ import annotations

from industrial_os3d.icon_groups.common import IconSpec
from industrial_os3d.icon_groups.common import (
    icon,
    lighten,
    subtract_mask,
    tone,
    union_masks,
)

ICON_GROUP = "tools_special"

OWNED_ICONS = (
    "lua_abacus_3d.png",
    "lua_hook_3d.png",
    "lua_snake_3d.png",
    "tool_factual_3d.png",
    "tool_mem_get_3d.png",
    "tool_mem_manage_3d.png",
    "tool_mem_search_3d.png",
    "tool_net_scan_3d.png",
    "tool_network_3d.png",
    "tool_office_cfg_3d.png",
    "tool_office_status_3d.png",
    "tool_proxy_3d.png",
    "tool_sensor_watch_3d.png",
    "tool_sys_ctrl_3d.png",
)

def _apply_extruded(scene_obj, mask, palette, role: str, depth: int = 10, glow: bool = False) -> None:
    top, bottom, side = tone(palette, role)
    scene_obj.render_extruded(mask, top, bottom, side, depth=depth)
    if glow:
        scene_obj.render_glow(mask, color=bottom, alpha=52, blur=14)


def _doc_sheet(scene_obj, x1: float, y1: float, x2: float, y2: float, fold: float = 16) -> tuple[object, object]:
    body = scene_obj.rounded_rect(x1, y1, x2, y2, 10)
    fold_cut = scene_obj.polygon([(x2 - fold, y1), (x2, y1), (x2, y1 + fold)])
    body = subtract_mask(body, fold_cut)
    fold_mask = scene_obj.polygon([(x2 - fold, y1), (x2, y1), (x2, y1 + fold)])
    return body, fold_mask


def _draw_text_lines(scene_obj, left: float, top: float, right: float, count: int, gap: float, width: float, role: str) -> None:
    _, color_bottom, _ = tone(scene_obj.palette, role)
    line_color = lighten(color_bottom, 0.04)
    for index in range(count):
        y = top + index * gap
        mask = scene_obj.line([(left, y), (right, y)], width)
        scene_obj.render_flat(mask, line_color, line_color)


def _render_badge(scene_obj, cx: float, cy: float, radius: float, role: str, depth: int = 6) -> None:
    mask = scene_obj.circle(cx, cy, radius)
    _apply_extruded(scene_obj, mask, scene_obj.palette, role, depth=depth)


def _render_check(scene_obj, x: float, y: float, scale: float, role: str) -> None:
    mask = scene_obj.line([(x, y + scale * 0.35), (x + scale * 0.28, y + scale * 0.65), (x + scale, y)], scale * 0.18)
    scene_obj.render_flat(mask, *tone(scene_obj.palette, role)[:2])


def _render_arrow_up(scene_obj, cx: float, cy: float, size: float, role: str) -> None:
    stem = scene_obj.rounded_rect(cx - size * 0.12, cy - size * 0.05, cx + size * 0.12, cy + size * 0.5, 16)
    head = scene_obj.polygon([(cx - size * 0.34, cy + size * 0.1), (cx, cy - size * 0.32), (cx + size * 0.34, cy + size * 0.1)])
    _apply_extruded(scene_obj, union_masks(stem, head), scene_obj.palette, role, depth=7)


def _render_sliders(scene_obj, x1: float, y1: float, x2: float, y2: float, role: str) -> None:
    bar1 = scene_obj.line([(x1, y1), (x2, y1)], 1.8)
    bar2 = scene_obj.line([(x1, (y1 + y2) / 2), (x2, (y1 + y2) / 2)], 1.8)
    bar3 = scene_obj.line([(x1, y2), (x2, y2)], 1.8)
    scene_obj.render_flat(union_masks(bar1, bar2, bar3), lighten(tone(scene_obj.palette, role)[1], 0.14), tone(scene_obj.palette, role)[1])
    knob1 = scene_obj.circle(x1 + (x2 - x1) * 0.72, y1, 2.1)
    knob2 = scene_obj.circle(x1 + (x2 - x1) * 0.32, (y1 + y2) / 2, 2.1)
    knob3 = scene_obj.circle(x1 + (x2 - x1) * 0.56, y2, 2.1)
    for knob in (knob1, knob2, knob3):
        _apply_extruded(scene_obj, knob, scene_obj.palette, role, depth=4)


def _render_lua_droplet(scene_obj, cx: float, cy: float, scale: float, role: str) -> None:
    circle = scene_obj.circle(cx, cy - scale * 0.15, scale * 0.36)
    tail = scene_obj.polygon([(cx - scale * 0.2, cy), (cx + scale * 0.2, cy), (cx, cy + scale * 0.42)])
    _apply_extruded(scene_obj, union_masks(circle, tail), scene_obj.palette, role, depth=8, glow=True)


def _render_node_globe(scene_obj, cx: float, cy: float, radius: float, role: str) -> None:
    sphere = scene_obj.circle(cx, cy, radius)
    _apply_extruded(scene_obj, sphere, scene_obj.palette, role, depth=10, glow=True)
    ring = scene_obj.arc_band(cx, cy, radius * 1.3, radius * 1.02, 30, 330)
    scene_obj.render_extruded(ring, *tone(scene_obj.palette, "accent"), depth=6)
    for px, py in ((cx - radius * 0.55, cy - radius * 0.35), (cx + radius * 0.48, cy - radius * 0.08), (cx - radius * 0.15, cy + radius * 0.5)):
        scene_obj.render_flat(scene_obj.circle(px, py, radius * 0.14), lighten(tone(scene_obj.palette, "accent")[0], 0.1), tone(scene_obj.palette, "accent")[0])


def _render_memory_tile(scene_obj, x1: float, y1: float, x2: float, y2: float, role: str, fold: float = 8) -> None:
    body = scene_obj.rounded_rect(x1, y1, x2, y2, 10)
    notch = scene_obj.polygon([(x2 - fold, y1), (x2, y1), (x2, y1 + fold)])
    body = subtract_mask(body, notch)
    _apply_extruded(scene_obj, body, scene_obj.palette, role, depth=8)
    scene_obj.render_flat(notch, lighten(tone(scene_obj.palette, role)[0], 0.08), tone(scene_obj.palette, role)[0])


def _render_gear_badge(scene_obj, cx: float, cy: float, radius: float, role: str) -> None:
    outer = scene_obj.circle(cx, cy, radius)
    inner = scene_obj.circle(cx, cy, radius * 0.45)
    ring = subtract_mask(outer, inner)
    teeth = []
    for dx, dy in ((0, -1), (0.7, -0.7), (1, 0), (0.7, 0.7), (0, 1), (-0.7, 0.7), (-1, 0), (-0.7, -0.7)):
        teeth.append(scene_obj.rounded_rect(cx + dx * radius * 0.72 - radius * 0.13, cy + dy * radius * 0.72 - radius * 0.13, cx + dx * radius * 0.72 + radius * 0.13, cy + dy * radius * 0.72 + radius * 0.13, 4))
    gear = union_masks(ring, *teeth)
    _apply_extruded(scene_obj, gear, scene_obj.palette, role, depth=6)


def _builder_factual(scene_obj, palette) -> None:
    sheet, fold = _doc_sheet(scene_obj, 18, 20, 70, 82, fold=14)
    _apply_extruded(scene_obj, sheet, palette, "neutral", depth=9)
    scene_obj.render_extruded(fold, *tone(palette, "secondary"), depth=5)
    _draw_text_lines(scene_obj, 26, 36, 58, 3, 8, 1.8, "neutral")
    seal = scene_obj.circle(63, 66, 8.5)
    _apply_extruded(scene_obj, seal, palette, "accent", depth=5, glow=True)
    _render_check(scene_obj, 58.6, 63.4, 10.5, "primary")
    badge = scene_obj.rounded_rect(54, 73, 69, 81, 4)
    scene_obj.render_flat(badge, lighten(tone(palette, "warm")[0], 0.08), tone(palette, "warm")[1])
    scene_obj.render_flat(scene_obj.circle(61.5, 77, 2.1), lighten(tone(palette, "primary")[0], 0.12), tone(palette, "primary")[1])


def _builder_mem_get(scene_obj, palette) -> None:
    back = scene_obj.rounded_rect(22, 36, 68, 76, 12)
    mid = scene_obj.rounded_rect(26, 28, 72, 68, 12)
    front = scene_obj.rounded_rect(30, 20, 76, 60, 12)
    _apply_extruded(scene_obj, back, palette, "secondary", depth=7)
    _apply_extruded(scene_obj, mid, palette, "primary", depth=7)
    _apply_extruded(scene_obj, front, palette, "warm", depth=8, glow=True)
    _render_arrow_up(scene_obj, 45, 47, 16, "accent")
    tab = scene_obj.rounded_rect(62, 15, 70, 25, 4)
    scene_obj.render_flat(tab, lighten(tone(palette, "accent")[0], 0.1), tone(palette, "accent")[0])


def _builder_mem_manage(scene_obj, palette) -> None:
    tiles = [
        (20, 33, 52, 76, "secondary"),
        (30, 24, 62, 68, "warm"),
        (40, 16, 72, 60, "primary"),
    ]
    for x1, y1, x2, y2, role in tiles:
        _render_memory_tile(scene_obj, x1, y1, x2, y2, role)
    _render_sliders(scene_obj, 26, 66, 56, 80, "accent")


def _builder_mem_search(scene_obj, palette) -> None:
    stack = scene_obj.rounded_rect(24, 34, 60, 76, 12)
    _apply_extruded(scene_obj, stack, palette, "secondary", depth=7)
    _draw_text_lines(scene_obj, 31, 45, 52, 3, 8, 1.8, "neutral")
    lens = scene_obj.circle(66, 58, 16)
    ring = subtract_mask(lens, scene_obj.circle(66, 58, 11))
    _apply_extruded(scene_obj, ring, palette, "primary", depth=7, glow=True)
    handle = scene_obj.rounded_rect(74, 67, 89, 75, 6)
    scene_obj.render_extruded(handle, *tone(palette, "warm"), depth=6)


def _builder_net_scan(scene_obj, palette) -> None:
    screen = scene_obj.circle(48, 49, 26)
    ring = subtract_mask(screen, scene_obj.circle(48, 49, 18))
    _apply_extruded(scene_obj, ring, palette, "secondary", depth=8, glow=True)
    sweep = scene_obj.arc_band(48, 49, 25, 7, 308, 26)
    scene_obj.render_flat(sweep, lighten(tone(palette, "accent")[0], 0.1), tone(palette, "accent")[1])
    for px, py in ((31, 39), (59, 33), (62, 61), (41, 69)):
        node = scene_obj.circle(px, py, 4.5)
        _apply_extruded(scene_obj, node, palette, "accent", depth=4)
    links = union_masks(
        scene_obj.line([(31, 39), (48, 49)], 1.4),
        scene_obj.line([(59, 33), (48, 49)], 1.4),
        scene_obj.line([(62, 61), (48, 49)], 1.4),
        scene_obj.line([(41, 69), (48, 49)], 1.4),
    )
    scene_obj.render_flat(links, lighten(tone(palette, "primary")[0], 0.1), tone(palette, "primary")[1])
    dish = scene_obj.polygon([(65, 80), (78, 55), (87, 81), (74, 88)])
    scene_obj.render_extruded(dish, *tone(palette, "warm"), depth=7)


def _builder_network(scene_obj, palette) -> None:
    _render_node_globe(scene_obj, 48, 49, 20, "warm")
    bridge = scene_obj.arc_band(48, 49, 30, 26, 24, 336)
    scene_obj.render_flat(bridge, lighten(tone(palette, "accent")[0], 0.04), tone(palette, "accent")[1])


def _builder_office_cfg(scene_obj, palette) -> None:
    tower = scene_obj.rounded_rect(26, 20, 68, 80, 10)
    side = scene_obj.rounded_rect(18, 30, 31, 80, 8)
    _apply_extruded(scene_obj, side, palette, "secondary", depth=6)
    _apply_extruded(scene_obj, tower, palette, "neutral", depth=10)
    for row in range(4):
        for col in range(3):
            window = scene_obj.rounded_rect(34 + col * 10, 30 + row * 11, 40 + col * 10, 35 + row * 11, 2)
            scene_obj.render_flat(window, lighten(tone(palette, "accent")[0], 0.15), tone(palette, "accent")[1])
    door = scene_obj.rounded_rect(43, 66, 51, 80, 3)
    scene_obj.render_flat(door, lighten(tone(palette, "warm")[0], 0.05), tone(palette, "warm")[1])
    _render_gear_badge(scene_obj, 73, 30, 9, "accent")


def _builder_office_status(scene_obj, palette) -> None:
    tower = scene_obj.rounded_rect(24, 22, 66, 80, 10)
    _apply_extruded(scene_obj, tower, palette, "secondary", depth=10, glow=True)
    for row in range(4):
        left = 32
        right = 58
        y = 31 + row * 11
        bar = scene_obj.line([(left, y), (right, y)], 2.0)
        scene_obj.render_flat(bar, lighten(tone(palette, "neutral")[0], 0.12), tone(palette, "neutral")[1])
    status = scene_obj.circle(70, 30, 8)
    _apply_extruded(scene_obj, status, palette, "accent", depth=5)
    _render_check(scene_obj, 65.2, 26.4, 8, "primary")


def _builder_proxy(scene_obj, palette) -> None:
    left = scene_obj.rounded_rect(20, 36, 36, 72, 9)
    right = scene_obj.rounded_rect(64, 36, 80, 72, 9)
    _apply_extruded(scene_obj, left, palette, "secondary", depth=7)
    _apply_extruded(scene_obj, right, palette, "primary", depth=7)
    bridge = scene_obj.rounded_rect(35, 48, 65, 60, 7)
    scene_obj.render_extruded(bridge, *tone(palette, "warm"), depth=7)
    arrow_l = scene_obj.polygon([(40, 54), (46, 49), (46, 52), (55, 52), (55, 56), (46, 56), (46, 59)])
    arrow_r = scene_obj.polygon([(60, 54), (54, 49), (54, 52), (45, 52), (45, 56), (54, 56), (54, 59)])
    scene_obj.render_flat(arrow_l, lighten(tone(palette, "accent")[0], 0.08), tone(palette, "accent")[1])
    scene_obj.render_flat(arrow_r, lighten(tone(palette, "accent")[0], 0.08), tone(palette, "accent")[1])


def _builder_sensor_watch(scene_obj, palette) -> None:
    module = scene_obj.rounded_rect(20, 27, 78, 77, 12)
    _apply_extruded(scene_obj, module, palette, "neutral", depth=9, glow=True)

    chip = scene_obj.rounded_rect(29, 34, 62, 67, 8)
    _apply_extruded(scene_obj, chip, palette, "primary", depth=7)

    pins = []
    for y in (40, 48, 56, 64):
        pins.append(scene_obj.line([(24, y), (30, y)], 2.0))
        pins.append(scene_obj.line([(61, y), (68, y)], 2.0))
    scene_obj.render_flat(union_masks(*pins), lighten(tone(palette, "neutral")[0], 0.08), tone(palette, "neutral")[1])

    eye = scene_obj.ellipse(45.5, 49.5, 15, 9)
    scene_obj.render_extruded(eye, *tone(palette, "secondary"), depth=5)
    pupil = scene_obj.circle(45.5, 49.5, 4.8)
    _apply_extruded(scene_obj, pupil, palette, "accent", depth=3)
    glint = scene_obj.circle(43, 47, 1.5)
    scene_obj.render_flat(glint, lighten(tone(palette, "neutral")[0], 0.2), lighten(tone(palette, "neutral")[0], 0.2))

    wave = scene_obj.line([(30, 70), (37, 70), (41, 63), (46, 75), (52, 66), (61, 66)], 2.4)
    scene_obj.render_flat(wave, lighten(tone(palette, "accent")[0], 0.12), tone(palette, "accent")[1])

    status = scene_obj.circle(69, 33, 6.0)
    _apply_extruded(scene_obj, status, palette, "warm", depth=4)
    live_dot = scene_obj.circle(69, 33, 2.2)
    scene_obj.render_flat(live_dot, lighten(tone(palette, "accent")[0], 0.16), tone(palette, "accent")[0])

    arcs = [
        scene_obj.arc_band(70, 33, 14, 11, 300, 40),
        scene_obj.arc_band(70, 33, 21, 18, 300, 40),
    ]
    for arc in arcs:
        scene_obj.render_flat(arc, lighten(tone(palette, "warm")[0], 0.1), tone(palette, "warm")[1])


def _builder_sys_ctrl(scene_obj, palette) -> None:
    board = scene_obj.rounded_rect(22, 24, 78, 78, 12)
    _apply_extruded(scene_obj, board, palette, "neutral", depth=10)
    knob_positions = [(34, 36), (62, 36), (34, 62), (62, 62)]
    for cx, cy in knob_positions:
        outer = scene_obj.circle(cx, cy, 8.5)
        _apply_extruded(scene_obj, outer, palette, "primary", depth=5)
        inner = scene_obj.circle(cx, cy, 3.2)
        scene_obj.render_flat(inner, lighten(tone(palette, "neutral")[0], 0.1), tone(palette, "neutral")[1])
        tick = scene_obj.line([(cx, cy - 6), (cx + 4, cy - 2)], 1.0)
        scene_obj.render_flat(tick, lighten(tone(palette, "accent")[0], 0.12), tone(palette, "accent")[1])
    lights = scene_obj.line([(31, 20), (69, 20)], 2.0)
    scene_obj.render_flat(lights, lighten(tone(palette, "warm")[0], 0.12), tone(palette, "warm")[1])


def _builder_lua_abacus(scene_obj, palette) -> None:
    frame = scene_obj.rounded_rect(25, 22, 74, 78, 8)
    _apply_extruded(scene_obj, frame, palette, "neutral", depth=9)
    for y in (36, 50, 64):
        bar = scene_obj.line([(31, y), (69, y)], 2.0)
        scene_obj.render_flat(bar, lighten(tone(palette, "accent")[0], 0.12), tone(palette, "accent")[1])
    for row, y in enumerate((34, 48, 62)):
        for offset, x in enumerate((36, 47, 58, 66)):
            bead = scene_obj.circle(x + (row % 2) * 3, y + (offset % 2) * 2, 4.2)
            _apply_extruded(scene_obj, bead, palette, "warm" if offset % 2 else "primary", depth=4)
    _render_lua_droplet(scene_obj, 60, 31, 18, "accent")


def _builder_lua_hook(scene_obj, palette) -> None:
    stem = scene_obj.rounded_rect(31, 20, 37, 54, 4)
    curve = scene_obj.arc_band(48, 56, 14, 8, 190, 350)
    cap = scene_obj.circle(34, 24, 4.2)
    mask = union_masks(stem, curve, cap)
    _apply_extruded(scene_obj, mask, palette, "secondary", depth=8, glow=True)
    pivot = scene_obj.circle(42, 56, 7)
    _apply_extruded(scene_obj, pivot, palette, "primary", depth=5)
    _render_lua_droplet(scene_obj, 58, 36, 15, "warm")


def _builder_lua_snake(scene_obj, palette) -> None:
    path = [(28, 74), (24, 61), (36, 48), (50, 53), (62, 44), (58, 31), (70, 22)]
    body = scene_obj.line(path, 17.0)
    joints = [scene_obj.circle(x, y, 8.5) for x, y in path[1:-1]]
    head = scene_obj.ellipse(70, 22, 15, 12)
    tail = scene_obj.circle(27, 75, 8)
    snake = union_masks(body, head, tail, *joints)
    _apply_extruded(scene_obj, snake, palette, "secondary", depth=11, glow=True)

    ridge = scene_obj.line([(29, 67), (37, 55), (49, 59), (58, 51), (63, 38), (72, 28)], 4.8)
    scene_obj.render_flat(ridge, lighten(tone(palette, "primary")[0], 0.16), tone(palette, "primary")[0])

    belly = scene_obj.line([(24, 61), (36, 48), (50, 53), (62, 44)], 3.0)
    scene_obj.render_flat(belly, lighten(tone(palette, "secondary")[0], 0.14), tone(palette, "secondary")[0])

    for px, py in ((36, 49), (50, 53), (61, 42)):
        spot = scene_obj.circle(px, py, 4.0)
        _apply_extruded(scene_obj, spot, palette, "accent", depth=4)
        scene_obj.render_flat(scene_obj.circle(px - 1.2, py - 1.4, 1.1), lighten(tone(palette, "neutral")[0], 0.18), tone(palette, "neutral")[0])

    eye = scene_obj.circle(73, 18, 2.1)
    scene_obj.render_flat(eye, lighten(tone(palette, "danger")[0], 0.18), tone(palette, "danger")[1])
    snout = scene_obj.circle(78, 25, 1.4)
    scene_obj.render_flat(snout, lighten(tone(palette, "neutral")[0], 0.18), tone(palette, "neutral")[0])
    tongue = scene_obj.line([(81, 24), (88, 19), (92, 23)], 2.2)
    scene_obj.render_flat(tongue, lighten(tone(palette, "warm")[0], 0.12), tone(palette, "warm")[1])


ICON_DEFINITIONS: list[IconSpec] = [
    icon("tool_factual_3d.png", "slate", _builder_factual),
    icon("tool_mem_get_3d.png", "crimson", _builder_mem_get),
    icon("tool_mem_manage_3d.png", "crimson", _builder_mem_manage),
    icon("tool_mem_search_3d.png", "crimson", _builder_mem_search),
    icon("tool_net_scan_3d.png", "teal", _builder_net_scan),
    icon("tool_network_3d.png", "sky", _builder_network),
    icon("tool_office_cfg_3d.png", "slate", _builder_office_cfg),
    icon("tool_office_status_3d.png", "slate", _builder_office_status),
    icon("tool_proxy_3d.png", "amber", _builder_proxy),
    icon("tool_sensor_watch_3d.png", "violet", _builder_sensor_watch),
    icon("tool_sys_ctrl_3d.png", "slate", _builder_sys_ctrl),
    icon("lua_abacus_3d.png", "violet", _builder_lua_abacus),
    icon("lua_hook_3d.png", "coral", _builder_lua_hook),
    icon("lua_snake_3d.png", "teal", _builder_lua_snake),
]
