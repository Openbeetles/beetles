from __future__ import annotations

from industrial_os3d.icon_groups.common import (
    IconSpec,
    Scene,
    darken,
    icon,
    lighten,
    outline_mask,
    subtract_mask,
    tone,
    union_masks,
)

ICON_GROUP = "dashboard_diag"

OWNED_ICONS = (
    "admission_3d.png",
    "bookmark_tabs_3d.png",
    "conversation_execution_3d.png",
    "dash_channels_3d.png",
    "dash_connection_3d.png",
    "device_info_3d.png",
    "device_unreachable_3d.png",
    "diag_delivery_3d.png",
    "diag_memory_3d.png",
    "diag_network_3d.png",
    "diag_voice_3d.png",
    "diagnose_3d.png",
    "execution_timing_3d.png",
    "faults_3d.png",
    "health_details_3d.png",
    "memory_3d.png",
    "reasoning_3d.png",
    "request_storage_3d.png",
    "runtime_3d.png",
    "storage_3d.png",
    "storage_media_3d.png",
    "voice_audio_3d.png",
)


def render_plate(
    scene: Scene,
    palette,
    x1: float,
    y1: float,
    x2: float,
    y2: float,
    role: str = "secondary",
    radius: float = 12,
    depth: int = 8,
    shadow_blur: int = 10,
) -> None:
    top, bottom, side = tone(palette, role)
    mask = scene.rounded_rect(x1, y1, x2, y2, radius)
    scene.render_extruded(mask, top, bottom, side, depth=depth, shadow_blur=shadow_blur)


def render_screen(scene: Scene, palette, x1: float, y1: float, x2: float, y2: float, role: str = "neutral") -> None:
    top, bottom, _ = tone(palette, role)
    mask = scene.rounded_rect(x1, y1, x2, y2, 10)
    scene.render_flat(mask, lighten(top, 0.08), darken(bottom, 0.02))
    rim = outline_mask(mask, scene.px(2))
    scene.render_flat(rim, lighten(palette.panel_top, 0.16), lighten(palette.panel_top, 0.04), alpha=92)


def render_line(scene: Scene, palette, points: list[tuple[float, float]], width: float, role: str = "accent") -> None:
    top, bottom, side = tone(palette, role)
    mask = scene.line(points, width)
    scene.render_extruded(mask, top, bottom, side, depth=4, shadow_blur=6, gloss=42)


def render_chip(
    scene: Scene,
    palette,
    x1: float,
    y1: float,
    x2: float,
    y2: float,
    role: str = "secondary",
    pins: int = 4,
) -> None:
    top, bottom, side = tone(palette, role)
    body = scene.rounded_rect(x1, y1, x2, y2, 10)
    scene.render_extruded(body, top, bottom, side, depth=8, shadow_blur=10)

    pin_top = y1 + 2.0
    pin_bottom = y2 - 2.0
    pin_width = max(1.8, (x2 - x1) / 42.0)
    for index in range(pins):
        t = (index + 1) / (pins + 1)
        px = x1 + (x2 - x1) * t
        left_pin = scene.line([(px - 11, pin_top), (px - 11, pin_bottom)], pin_width)
        right_pin = scene.line([(px + 11, pin_top), (px + 11, pin_bottom)], pin_width)
        scene.render_flat(left_pin, lighten(palette.panel_top, 0.02), darken(palette.panel_bottom, 0.12), alpha=92)
        scene.render_flat(right_pin, lighten(palette.panel_top, 0.02), darken(palette.panel_bottom, 0.12), alpha=92)


def render_magnifier(scene: Scene, palette, cx: float, cy: float, radius: float, role: str = "accent") -> None:
    top, bottom, side = tone(palette, role)
    lens = scene.circle(cx, cy, radius)
    scene.render_extruded(lens, top, bottom, side, depth=6, shadow_blur=8)
    handle = scene.line([(cx + radius * 0.62, cy + radius * 0.62), (cx + radius * 1.7, cy + radius * 1.7)], 4.6)
    scene.render_extruded(handle, top, bottom, side, depth=4, shadow_blur=6)


def render_warning_triangle(scene: Scene, palette, points: list[tuple[float, float]], role: str = "danger") -> None:
    top, bottom, side = tone(palette, role)
    mask = scene.polygon(points)
    scene.render_extruded(mask, top, bottom, side, depth=8, shadow_blur=12)


def render_stopwatch(scene: Scene, palette, role: str = "accent") -> None:
    top, bottom, side = tone(palette, role)
    ring = subtract_mask(scene.circle(50, 38, 17), scene.circle(50, 38, 11))
    scene.render_extruded(ring, top, bottom, side, depth=7, shadow_blur=8)
    crown = scene.rounded_rect(45.5, 17, 54.5, 23.5, 3)
    scene.render_extruded(crown, lighten(top, 0.04), bottom, side, depth=5, shadow_blur=5)
    stem = scene.rounded_rect(48.5, 23, 51.5, 28.5, 2)
    scene.render_flat(stem, lighten(palette.panel_top, 0.08), darken(palette.panel_bottom, 0.04))
    hand = scene.line([(50, 38), (61, 31)], 3.2)
    scene.render_extruded(hand, palette.accent_top, palette.accent_bottom, darken(palette.accent_bottom, 0.3), depth=4, shadow_blur=4)
    pulse = scene.line([(27, 62), (37, 62), (42, 54), (47, 67), (54, 47), (61, 62), (72, 62)], 3.2)
    scene.render_flat(pulse, palette.accent_top, palette.accent_bottom)


def render_radar(scene: Scene, palette, role: str = "secondary") -> None:
    top, bottom, side = tone(palette, role)
    outer = scene.circle(50, 40, 20)
    inner = scene.circle(50, 40, 12)
    ring = subtract_mask(outer, inner)
    scene.render_extruded(ring, top, bottom, side, depth=6, shadow_blur=8)
    sweep = union_masks(scene.arc_band(50, 40, 20, 13.5, 300, 360), scene.arc_band(50, 40, 20, 13.5, 0, 20))
    scene.render_flat(sweep, palette.accent_top, palette.accent_bottom)
    for point in ((36, 53), (64, 52), (70, 34)):
        node = scene.circle(point[0], point[1], 3.2)
        scene.render_extruded(node, palette.primary_top, palette.primary_bottom, darken(palette.primary_bottom, 0.24), depth=3, shadow_blur=4)
    render_line(scene, palette, [(36, 53), (50, 40), (64, 52)], 2.8, "accent")
    render_line(scene, palette, [(64, 52), (70, 34)], 2.4, "secondary")


def render_dashboard_card(scene: Scene, palette, x1: float, y1: float, x2: float, y2: float, role: str = "secondary") -> None:
    render_plate(scene, palette, x1, y1, x2, y2, role=role, radius=14, depth=8, shadow_blur=11)
    render_screen(scene, palette, x1 + 4, y1 + 4, x2 - 4, y2 - 6)


def render_device_body(scene: Scene, palette, role: str = "secondary") -> None:
    render_plate(scene, palette, 21, 20, 79, 74, role=role, radius=13, depth=8, shadow_blur=10)
    screen = scene.rounded_rect(27, 27, 73, 64, 9)
    scene.render_flat(screen, lighten(palette.panel_top, 0.08), darken(palette.panel_bottom, 0.05))
    slot = scene.rounded_rect(39, 21.5, 61, 28, 4)
    scene.render_flat(slot, darken(palette.shell_bottom, 0.02), darken(palette.shell_bottom, 0.14))


def render_chip_with_lens(scene: Scene, palette, lens_cx: float, lens_cy: float, lens_r: float, role: str = "secondary") -> None:
    render_chip(scene, palette, 23, 24, 73, 70, role=role, pins=4)
    render_magnifier(scene, palette, lens_cx, lens_cy, lens_r, "accent")


def render_delivery_report(scene: Scene, palette) -> None:
    report = scene.rounded_rect(20, 24, 62, 78, 9)
    scene.render_extruded(
        report,
        lighten(palette.panel_top, 0.06),
        darken(palette.panel_bottom, 0.04),
        darken(palette.panel_bottom, 0.2),
        depth=8,
        shadow_blur=10,
        gloss=54,
    )

    clip = scene.rounded_rect(30, 18, 52, 29, 5)
    scene.render_extruded(clip, palette.secondary_top, palette.secondary_bottom, darken(palette.secondary_bottom, 0.24), depth=5, shadow_blur=5)
    header = scene.rounded_rect(26, 32, 56, 38, 3)
    scene.render_flat(header, palette.primary_top, palette.primary_bottom)

    pulse = scene.line([(27, 54), (34, 54), (38, 47), (43, 62), (49, 50), (57, 50)], 2.6)
    scene.render_flat(pulse, palette.accent_top, palette.accent_bottom)
    row_a = scene.rounded_rect(27, 43, 52, 46, 1.5)
    row_b = scene.rounded_rect(27, 66, 46, 69, 1.5)
    scene.render_flat(union_masks(row_a, row_b), darken(palette.shell_bottom, 0.02), darken(palette.shell_bottom, 0.14), alpha=118)

    badge = scene.circle(54, 66, 6.0)
    scene.render_extruded(badge, palette.accent_top, palette.accent_bottom, darken(palette.accent_bottom, 0.24), depth=4, shadow_blur=5, gloss=40)
    check = scene.line([(50.5, 66), (53.5, 69), (59.5, 61.5)], 2.2)
    scene.render_flat(check, palette.panel_top, palette.panel_bottom)


def render_memory_card(scene: Scene, palette, role: str = "secondary") -> None:
    top, bottom, side = tone(palette, role)
    body = scene.rounded_rect(24, 24, 76, 70, 10)
    scene.render_extruded(body, top, bottom, side, depth=8, shadow_blur=10)
    connector = scene.rounded_rect(27, 20, 71, 31, 4)
    scene.render_flat(connector, lighten(palette.panel_top, 0.08), darken(palette.panel_bottom, 0.08))
    slot = scene.rounded_rect(58, 27, 68, 59, 3)
    scene.render_flat(slot, darken(palette.shell_bottom, 0.12), darken(palette.shell_bottom, 0.28))
    for y in (38, 44, 50):
        render_line(scene, palette, [(31, y), (52, y)], 2.2, "accent")


def render_storage_stack(scene: Scene, palette, role: str = "secondary") -> None:
    top, bottom, side = tone(palette, role)
    lower = scene.rounded_rect(22, 44, 78, 73, 11)
    upper = scene.rounded_rect(28, 26, 72, 49, 10)
    scene.render_extruded(lower, top, bottom, side, depth=8, shadow_blur=10)
    scene.render_extruded(upper, lighten(top, 0.04), bottom, side, depth=6, shadow_blur=8)
    bay = scene.rounded_rect(33, 50, 67, 58, 4)
    scene.render_flat(bay, lighten(palette.panel_top, 0.12), darken(palette.panel_bottom, 0.08))
    led = scene.circle(66, 38, 2.1)
    scene.render_extruded(led, palette.accent_top, palette.accent_bottom, darken(palette.accent_bottom, 0.22), depth=2, shadow_blur=3)


def render_voice_monitor(scene: Scene, palette, role: str = "secondary") -> None:
    top, bottom, side = tone(palette, role)
    head = scene.rounded_rect(39, 18, 61, 54, 11)
    scene.render_extruded(head, top, bottom, side, depth=8, shadow_blur=10)
    for y in (31, 38, 45):
        notch = scene.rounded_rect(43, y - 1.2, 57, y + 1.2, 1.2)
        scene.render_flat(notch, darken(palette.shell_bottom, 0.22), darken(palette.shell_bottom, 0.08))
    stem = scene.rounded_rect(45, 53, 55, 70, 5)
    scene.render_extruded(stem, lighten(top, 0.02), bottom, side, depth=5, shadow_blur=6)
    base = scene.rounded_rect(31, 67, 69, 78, 6)
    scene.render_extruded(base, lighten(top, 0.05), bottom, side, depth=5, shadow_blur=6)
    for band in (1, 2, 3):
        arc = union_masks(
            scene.arc_band(64, 36, 10 + band * 5, 8 + band * 5, 300, 360),
            scene.arc_band(64, 36, 10 + band * 5, 8 + band * 5, 0, 45),
        )
        scene.render_flat(arc, palette.primary_top, palette.primary_bottom)


def render_device_card(scene: Scene, palette, role: str = "secondary") -> None:
    render_device_body(scene, palette, role=role)
    info = scene.circle(67, 36, 6.6)
    scene.render_extruded(info, palette.primary_top, palette.primary_bottom, darken(palette.primary_bottom, 0.24), depth=4, shadow_blur=5)
    info_dot = scene.circle(67, 32.5, 1.8)
    scene.render_flat(info_dot, palette.panel_top, palette.panel_bottom)
    info_bar = scene.line([(67, 36.2), (67, 42.5)], 2.1)
    scene.render_flat(info_bar, palette.panel_top, palette.panel_bottom)
    render_line(scene, palette, [(31, 52), (53, 52)], 2.6, "accent")
    render_line(scene, palette, [(31, 59), (48, 59)], 2.3, "secondary")


def render_network_device(scene: Scene, palette, unreachable: bool = False) -> None:
    role = "danger" if unreachable else "secondary"
    render_device_body(scene, palette, role=role)
    tower = scene.line([(50, 23), (50, 12)], 4)
    scene.render_extruded(tower, palette.accent_top, palette.accent_bottom, darken(palette.accent_bottom, 0.22), depth=5, shadow_blur=6)
    for start, end, outer, inner in ((11, 18, 22, 17), (12, 24, 28, 21), (13, 30, 34, 25)):
        arc = scene.arc_band(50, 24, outer, inner, start, end)
        scene.render_flat(arc, palette.accent_top if not unreachable else palette.danger_top, palette.accent_bottom if not unreachable else palette.danger_bottom)
    if unreachable:
        cross_a = scene.line([(34, 32), (66, 64)], 4.4)
        cross_b = scene.line([(66, 32), (34, 64)], 4.4)
        scene.render_extruded(cross_a, palette.danger_top, palette.danger_bottom, darken(palette.danger_bottom, 0.24), depth=4, shadow_blur=5)
        scene.render_extruded(cross_b, palette.danger_top, palette.danger_bottom, darken(palette.danger_bottom, 0.24), depth=4, shadow_blur=5)
        alert = scene.polygon([(77, 20), (88, 40), (66, 40)])
        scene.render_extruded(alert, palette.danger_top, palette.danger_bottom, darken(palette.danger_bottom, 0.24), depth=5, shadow_blur=7)
        bang = scene.line([(77, 26), (77, 33)], 2.8)
        scene.render_flat(bang, palette.panel_top, palette.panel_bottom)
        dot = scene.circle(77, 36, 1.8)
        scene.render_flat(dot, palette.panel_top, palette.panel_bottom)


def build_bookmark_tabs(scene: Scene, palette) -> None:
    render_dashboard_card(scene, palette, 18, 21, 82, 74, "slate")
    for x1, x2, role in ((24, 38, "primary"), (37, 51, "secondary"), (50, 64, "accent")):
        tab = scene.rounded_rect(x1, 15, x2, 28, 5)
        top, bottom, side = tone(palette, role)
        scene.render_extruded(tab, top, bottom, side, depth=5, shadow_blur=6)
    ribbon = scene.polygon([(68, 27), (75, 31), (75, 59), (71.5, 55), (68, 59)])
    top, bottom, side = tone(palette, "warm")
    scene.render_extruded(ribbon, top, bottom, side, depth=6, shadow_blur=7)
    render_line(scene, palette, [(28, 44), (46, 44)], 2.6, "secondary")
    render_line(scene, palette, [(28, 51), (44, 51)], 2.3, "secondary")
    render_line(scene, palette, [(28, 58), (40, 58)], 2.1, "accent")


def build_admission(scene: Scene, palette) -> None:
    shield = scene.polygon([(50, 15), (75, 26), (70, 59), (50, 80), (30, 59), (25, 26)])
    top, bottom, side = tone(palette, "secondary")
    scene.render_extruded(shield, top, bottom, side, depth=9, shadow_blur=12, gloss=60)

    gate = scene.rounded_rect(37, 31, 63, 61, 7)
    scene.render_flat(gate, lighten(palette.panel_top, 0.08), darken(palette.panel_bottom, 0.06))
    for x in (42, 50, 58):
        bar = scene.rounded_rect(x - 1.5, 35, x + 1.5, 58, 1.6)
        scene.render_flat(bar, darken(palette.shell_bottom, 0.04), darken(palette.shell_bottom, 0.18), alpha=124)

    check = scene.line([(39, 64), (47, 72), (64, 48)], 4.0)
    scene.render_extruded(check, palette.accent_top, palette.accent_bottom, darken(palette.accent_bottom, 0.24), depth=4, shadow_blur=5)
    node = scene.circle(69, 32, 4.0)
    scene.render_extruded(node, palette.primary_top, palette.primary_bottom, darken(palette.primary_bottom, 0.24), depth=3, shadow_blur=4)


def build_conversation_execution(scene: Scene, palette) -> None:
    bubble = scene.rounded_rect(18, 25, 73, 62, 13)
    tail = scene.polygon([(30, 60), (25, 75), (43, 62)])
    top, bottom, side = tone(palette, "secondary")
    scene.render_extruded(union_masks(bubble, tail), top, bottom, side, depth=8, shadow_blur=10, gloss=58)

    small = scene.rounded_rect(49, 48, 83, 74, 10)
    small_tail = scene.polygon([(74, 72), (82, 83), (68, 73)])
    top2, bottom2, side2 = tone(palette, "primary")
    scene.render_extruded(union_masks(small, small_tail), top2, bottom2, side2, depth=7, shadow_blur=9, gloss=54)

    for x in (32, 45, 58):
        dot = scene.circle(x, 43, 2.6)
        scene.render_flat(dot, lighten(palette.panel_top, 0.06), darken(palette.panel_bottom, 0.04), alpha=210)

    path = scene.line([(35, 64), (48, 69), (62, 61)], 2.4)
    scene.render_flat(path, palette.accent_top, palette.accent_bottom)
    endpoint = scene.circle(62, 61, 3.2)
    scene.render_extruded(endpoint, palette.accent_top, palette.accent_bottom, darken(palette.accent_bottom, 0.22), depth=3, shadow_blur=4)


def build_dash_channels(scene: Scene, palette) -> None:
    left = scene.rounded_rect(17, 31, 39, 52, 7)
    right = scene.rounded_rect(61, 27, 83, 48, 7)
    bottom = scene.rounded_rect(37, 64, 63, 82, 7)
    hub = scene.rounded_rect(38, 37, 62, 61, 9)

    render_line(scene, palette, [(39, 43), (50, 49), (61, 39)], 4.2, "accent")
    render_line(scene, palette, [(50, 57), (50, 66)], 4.0, "secondary")

    for mask, role in ((left, "sky"), (right, "secondary"), (bottom, "teal")):
        top, bottom_color, side = tone(palette, role)
        scene.render_extruded(mask, top, bottom_color, side, depth=7, shadow_blur=9, gloss=52)

    top, bottom_color, side = tone(palette, "primary")
    scene.render_extruded(hub, top, bottom_color, side, depth=8, shadow_blur=10, gloss=56)

    hub_screen = scene.rounded_rect(43, 43, 57, 55, 4)
    scene.render_flat(hub_screen, darken(palette.shell_bottom, 0.08), darken(palette.shell_bottom, 0.2))

    status_masks = union_masks(
        scene.circle(24, 38, 2.0),
        scene.circle(68, 34, 2.0),
        scene.circle(44, 70, 2.0),
        scene.circle(56, 70, 2.0),
    )
    scene.render_flat(status_masks, palette.accent_top, palette.accent_bottom)

    for x1, x2, y in ((27, 34, 45), (70, 79, 41), (43, 57, 76)):
        stripe = scene.rounded_rect(x1, y - 1.3, x2, y + 1.3, 1.3)
        scene.render_flat(stripe, lighten(palette.panel_top, 0.08), darken(palette.panel_bottom, 0.04), alpha=160)

    hub_dot = scene.circle(50, 49, 3.1)
    scene.render_extruded(hub_dot, palette.accent_top, palette.accent_bottom, darken(palette.accent_bottom, 0.22), depth=3, shadow_blur=4)


def build_dash_connection(scene: Scene, palette) -> None:
    render_dashboard_card(scene, palette, 18, 20, 82, 74, "sky")
    left_outer = scene.circle(39, 42, 11)
    left_inner = scene.circle(39, 42, 6.2)
    right_outer = scene.circle(61, 42, 11)
    right_inner = scene.circle(61, 42, 6.2)
    left_ring = subtract_mask(left_outer, left_inner)
    right_ring = subtract_mask(right_outer, right_inner)
    scene.render_extruded(left_ring, palette.primary_top, palette.primary_bottom, darken(palette.primary_bottom, 0.24), depth=6, shadow_blur=7)
    scene.render_extruded(right_ring, palette.secondary_top, palette.secondary_bottom, darken(palette.secondary_bottom, 0.24), depth=6, shadow_blur=7)
    bridge = scene.rounded_rect(45, 38, 55, 46, 3)
    scene.render_extruded(bridge, palette.accent_top, palette.accent_bottom, darken(palette.accent_bottom, 0.24), depth=4, shadow_blur=5)
    render_line(scene, palette, [(32, 58), (50, 58), (68, 58)], 2.4, "accent")
    render_line(scene, palette, [(72, 30), (78, 24)], 2.4, "secondary")
    tail = union_masks(scene.arc_band(71, 28, 10, 7.2, 300, 360), scene.arc_band(71, 28, 10, 7.2, 0, 42))
    scene.render_flat(tail, palette.secondary_top, palette.secondary_bottom)


def build_device_info(scene: Scene, palette) -> None:
    render_device_card(scene, palette, "slate")
    badge = scene.rounded_rect(60, 25, 72, 37, 4)
    top, bottom, side = tone(palette, "accent")
    scene.render_extruded(badge, top, bottom, side, depth=4, shadow_blur=5)
    dot = scene.circle(66, 31, 1.7)
    scene.render_flat(dot, palette.panel_top, palette.panel_bottom)


def build_device_unreachable(scene: Scene, palette) -> None:
    render_network_device(scene, palette, unreachable=True)


def build_diag_delivery(scene: Scene, palette) -> None:
    render_delivery_report(scene, palette)

    endpoint = scene.circle(82, 35, 8.0)
    scene.render_extruded(endpoint, palette.secondary_top, palette.secondary_bottom, darken(palette.secondary_bottom, 0.24), depth=4, shadow_blur=5, gloss=36)

    route = scene.line([(59, 56), (70, 48), (82, 36)], 5.2)
    arrow_head = scene.polygon([(79, 27), (90, 34), (80, 44)])
    top, bottom, side = tone(palette, "accent")
    scene.render_extruded(union_masks(route, arrow_head), top, bottom, side, depth=5, shadow_blur=7, gloss=48)


def build_diag_memory(scene: Scene, palette) -> None:
    render_chip_with_lens(scene, palette, 67, 32, 9.5, "violet")
    render_line(scene, palette, [(30, 52), (42, 52), (48, 45)], 2.2, "accent")
    render_line(scene, palette, [(55, 52), (63, 52)], 2.0, "secondary")


def build_diag_network(scene: Scene, palette) -> None:
    render_radar(scene, palette, "teal")
    sweep = scene.rounded_rect(24, 66, 76, 72, 4)
    scene.render_flat(sweep, palette.panel_top, palette.panel_bottom)


def build_diag_voice(scene: Scene, palette) -> None:
    render_voice_monitor(scene, palette, "coral")
    ring = scene.circle(72, 28, 5.5)
    scene.render_flat(ring, palette.accent_top, palette.accent_bottom)


def build_diagnose(scene: Scene, palette) -> None:
    render_dashboard_card(scene, palette, 17, 21, 83, 74, "slate")
    render_magnifier(scene, palette, 64, 41, 10.5, "accent")
    gauge = scene.arc_band(40, 44, 13, 9, 195, 335)
    scene.render_flat(gauge, palette.secondary_top, palette.secondary_bottom)
    needle = scene.line([(40, 44), (49, 36)], 2.8)
    scene.render_flat(needle, palette.primary_top, palette.primary_bottom)
    render_line(scene, palette, [(26, 58), (45, 58)], 2.2, "secondary")
    render_line(scene, palette, [(26, 65), (39, 65)], 2.0, "accent")


def build_execution_timing(scene: Scene, palette) -> None:
    render_stopwatch(scene, palette, "amber")
    lanes = union_masks(
        scene.rounded_rect(20, 65, 37, 70, 2.4),
        scene.rounded_rect(43, 65, 61, 70, 2.4),
        scene.rounded_rect(67, 65, 82, 70, 2.4),
    )
    scene.render_flat(lanes, lighten(palette.panel_top, 0.06), darken(palette.panel_bottom, 0.06), alpha=150)
    spark = scene.circle(75, 29, 4.0)
    scene.render_extruded(spark, palette.accent_top, palette.accent_bottom, darken(palette.accent_bottom, 0.24), depth=3, shadow_blur=4)


def build_faults(scene: Scene, palette) -> None:
    render_warning_triangle(scene, palette, [(50, 18), (83, 76), (17, 76)], "danger")
    bar = scene.rounded_rect(47, 34, 53, 55, 2.5)
    scene.render_flat(bar, palette.panel_top, palette.panel_bottom)
    dot = scene.circle(50, 62, 2.2)
    scene.render_flat(dot, palette.panel_top, palette.panel_bottom)
    crack = scene.line([(34, 62), (44, 55), (54, 60), (65, 48)], 2.7)
    scene.render_flat(crack, palette.panel_top, palette.panel_bottom)
    spark = scene.circle(73, 28, 3.2)
    scene.render_extruded(spark, palette.accent_top, palette.accent_bottom, darken(palette.accent_bottom, 0.24), depth=3, shadow_blur=4)


def build_health_details(scene: Scene, palette) -> None:
    render_dashboard_card(scene, palette, 18, 22, 82, 76, "mint")

    heart = union_masks(
        scene.circle(41, 43, 7.2),
        scene.circle(54, 43, 7.2),
        scene.polygon([(33, 45), (62, 45), (48, 67)]),
    )
    scene.render_extruded(
        heart,
        palette.danger_top,
        palette.danger_bottom,
        darken(palette.danger_bottom, 0.24),
        depth=6,
        shadow_blur=7,
        gloss=50,
    )
    pulse = scene.line([(25, 56), (36, 56), (40, 50), (45, 62), (52, 39), (58, 56), (72, 56)], 3.0)
    scene.render_flat(pulse, palette.accent_top, palette.accent_bottom)
    badge = scene.circle(69, 32, 5.2)
    scene.render_extruded(badge, palette.secondary_top, palette.secondary_bottom, darken(palette.secondary_bottom, 0.24), depth=4, shadow_blur=5)


def build_memory(scene: Scene, palette) -> None:
    render_memory_card(scene, palette, "violet")


def build_reasoning(scene: Scene, palette) -> None:
    render_chip(scene, palette, 22, 23, 78, 74, role="violet", pins=5)
    nodes = [
        (37, 39, "primary"),
        (54, 32, "secondary"),
        (65, 47, "accent"),
        (45, 60, "warm"),
        (59, 65, "primary"),
    ]
    for a, b in ((0, 1), (1, 2), (0, 3), (3, 4), (2, 4)):
        render_line(scene, palette, [(nodes[a][0], nodes[a][1]), (nodes[b][0], nodes[b][1])], 2.2, "accent")
    for x, y, role in nodes:
        top, bottom, side = tone(palette, role)
        scene.render_extruded(scene.circle(x, y, 4.2), top, bottom, side, depth=4, shadow_blur=5)
    core = scene.circle(50, 48, 6.5)
    scene.render_flat(core, lighten(palette.panel_top, 0.12), darken(palette.panel_bottom, 0.04), alpha=180)


def build_request_storage(scene: Scene, palette) -> None:
    request = scene.rounded_rect(16, 23, 58, 53, 9)
    top, bottom, side = tone(palette, "secondary")
    scene.render_extruded(request, top, bottom, side, depth=7, shadow_blur=9, gloss=56)
    for y, width in ((32, 24), (40, 17), (48, 22)):
        line = scene.rounded_rect(23, y - 1.3, 23 + width, y + 1.3, 1.3)
        scene.render_flat(line, lighten(palette.panel_top, 0.1), darken(palette.panel_bottom, 0.05), alpha=175)

    arrow = scene.line([(52, 48), (64, 57), (75, 57)], 4.0)
    head = scene.polygon([(72, 49), (84, 57), (72, 65)])
    top2, bottom2, side2 = tone(palette, "accent")
    scene.render_extruded(union_masks(arrow, head), top2, bottom2, side2, depth=4, shadow_blur=5)

    render_storage_stack(scene, palette, "slate")
    led = scene.circle(76, 37, 3.2)
    scene.render_extruded(led, palette.primary_top, palette.primary_bottom, darken(palette.primary_bottom, 0.24), depth=3, shadow_blur=4)


def build_runtime(scene: Scene, palette) -> None:
    render_stopwatch(scene, palette, "amber")


def build_storage(scene: Scene, palette) -> None:
    render_storage_stack(scene, palette, "slate")


def build_storage_media(scene: Scene, palette) -> None:
    card = scene.polygon([(28, 18), (66, 18), (78, 31), (78, 78), (28, 78)])
    top, bottom, side = tone(palette, "secondary")
    scene.render_extruded(card, top, bottom, side, depth=8, shadow_blur=10, gloss=58)
    notch = scene.rounded_rect(57, 19, 71, 34, 3)
    scene.render_flat(notch, darken(palette.shell_bottom, 0.06), darken(palette.shell_bottom, 0.18), alpha=138)
    chip = scene.rounded_rect(34, 27, 53, 44, 5)
    scene.render_extruded(chip, palette.accent_top, palette.accent_bottom, darken(palette.accent_bottom, 0.24), depth=4, shadow_blur=5)
    for y in (54, 62, 70):
        render_line(scene, palette, [(36, y), (67, y)], 2.4, "neutral")
    status = scene.circle(69, 44, 3.4)
    scene.render_extruded(status, palette.primary_top, palette.primary_bottom, darken(palette.primary_bottom, 0.24), depth=3, shadow_blur=4)


def build_voice_audio(scene: Scene, palette) -> None:
    render_voice_monitor(scene, palette, "coral")
    speaker = scene.polygon([(64, 55), (74, 48), (74, 72), (64, 65)])
    scene.render_extruded(speaker, palette.secondary_top, palette.secondary_bottom, darken(palette.secondary_bottom, 0.24), depth=5, shadow_blur=6)
    for radius in (10, 16):
        wave = union_masks(
            scene.arc_band(76, 60, radius, radius - 2.6, 315, 360),
            scene.arc_band(76, 60, radius, radius - 2.6, 0, 45),
        )
        scene.render_flat(wave, palette.accent_top, palette.accent_bottom)


ICON_DEFINITIONS: list[IconSpec] = [
    icon("admission_3d.png", "mint", build_admission),
    icon("bookmark_tabs_3d.png", "slate", build_bookmark_tabs),
    icon("conversation_execution_3d.png", "sky", build_conversation_execution),
    icon("dash_channels_3d.png", "teal", build_dash_channels),
    icon("dash_connection_3d.png", "sky", build_dash_connection),
    icon("device_info_3d.png", "slate", build_device_info),
    icon("device_unreachable_3d.png", "crimson", build_device_unreachable),
    icon("diag_delivery_3d.png", "amber", build_diag_delivery),
    icon("diag_memory_3d.png", "violet", build_diag_memory),
    icon("diag_network_3d.png", "teal", build_diag_network),
    icon("diag_voice_3d.png", "coral", build_diag_voice),
    icon("diagnose_3d.png", "slate", build_diagnose),
    icon("execution_timing_3d.png", "amber", build_execution_timing),
    icon("faults_3d.png", "crimson", build_faults),
    icon("health_details_3d.png", "mint", build_health_details),
    icon("memory_3d.png", "violet", build_memory),
    icon("reasoning_3d.png", "violet", build_reasoning),
    icon("request_storage_3d.png", "teal", build_request_storage),
    icon("runtime_3d.png", "amber", build_runtime),
    icon("storage_3d.png", "slate", build_storage),
    icon("storage_media_3d.png", "slate", build_storage_media),
    icon("voice_audio_3d.png", "coral", build_voice_audio),
]
