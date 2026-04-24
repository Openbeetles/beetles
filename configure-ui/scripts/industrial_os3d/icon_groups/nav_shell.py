from __future__ import annotations

from PIL import Image, ImageDraw

from industrial_os3d.icon_groups.common import (
    IconSpec,
    Palette,
    Scene,
    blank_mask,
    darken,
    icon,
    intersect_mask,
    lighten,
    outline_mask,
    shift_mask,
    subtract_mask,
    tone,
    union_masks,
    with_alpha,
)

ICON_GROUP = "nav_shell"

OWNED_ICONS = (
    "accounts_3d.png",
    "bookmark_3d.png",
    "bot_3d.png",
    "chat_3d.png",
    "devices_3d.png",
    "globe_3d.png",
    "history_3d.png",
    "home_3d.png",
    "link_3d.png",
    "power_3d.png",
    "puzzle_3d.png",
    "safe_3d.png",
    "settings_3d.png",
    "skills_3d.png",
    "strategy_3d.png",
    "system_logs_3d.png",
    "theme_3d.png",
    "tools_3d.png",
    "unsaved_changes_3d.png",
)


def _rect(scene: Scene, x1: float, y1: float, x2: float, y2: float) -> Image.Image:
    mask = blank_mask(scene.size)
    ImageDraw.Draw(mask).rectangle(scene.box(x1, y1, x2, y2), fill=255)
    return mask


def _rounded_rect(scene: Scene, x1: float, y1: float, x2: float, y2: float, radius: float) -> Image.Image:
    mask = blank_mask(scene.size)
    ImageDraw.Draw(mask).rounded_rectangle(scene.box(x1, y1, x2, y2), radius=scene.scalar(radius), fill=255)
    return mask


def _polygon(scene: Scene, points: list[tuple[float, float]]) -> Image.Image:
    mask = blank_mask(scene.size)
    ImageDraw.Draw(mask).polygon([scene.pt(x, y) for x, y in points], fill=255)
    return mask


def _ellipse(scene: Scene, cx: float, cy: float, rx: float, ry: float) -> Image.Image:
    mask = blank_mask(scene.size)
    px, py = scene.pt(cx, cy)
    rx_px = scene.scalar(rx)
    ry_px = scene.scalar(ry)
    ImageDraw.Draw(mask).ellipse((px - rx_px, py - ry_px, px + rx_px, py + ry_px), fill=255)
    return mask


def _line(scene: Scene, points: list[tuple[float, float]], width: float) -> Image.Image:
    mask = blank_mask(scene.size)
    ImageDraw.Draw(mask).line([scene.pt(x, y) for x, y in points], fill=255, width=scene.scalar(width), joint="curve")
    return mask


def _paint(scene: Scene, mask, color) -> None:
    scene.render_flat(mask, color, color)


def _extrude(
    scene: Scene,
    palette: Palette,
    mask,
    role: str = "primary",
    depth: int = 7,
    shadow_offset: tuple[int, int] = (0, 8),
    shadow_blur: int = 10,
    gloss: int = 54,
) -> None:
    top, bottom, side = tone(palette, role)  # type: ignore[arg-type]
    scene.render_extruded(
        mask,
        top,
        bottom,
        side,
        depth=depth,
        shadow=with_alpha(palette.shadow, 98),
        shadow_offset=shadow_offset,
        shadow_blur=shadow_blur,
        gloss=gloss,
    )


def _clipped_line(scene: Scene, clip, points: list[tuple[float, float]], width: float):
    return intersect_mask(_line(scene, points, width), clip)


def build_bookmark(scene: Scene, palette: Palette) -> None:
    body = _polygon(
        scene,
        [
            (25, 20),
            (75, 20),
            (75, 72),
            (48, 92),
            (25, 72),
        ],
    )
    _extrude(scene, palette, body, role="warm", depth=8, shadow_offset=(0, 9), shadow_blur=9, gloss=62)
    fold = _polygon(scene, [(58, 24), (75, 24), (75, 57), (66, 52), (58, 58)])
    _paint(scene, fold, darken(tone(palette, "warm")[1], 0.18))
    spine = _rect(scene, 34, 29, 41, 76)
    _paint(scene, spine, lighten(tone(palette, "warm")[0], 0.16))
    crease = _rect(scene, 49, 28, 54, 78)
    _paint(scene, crease, with_alpha(darken(tone(palette, "warm")[1], 0.22), 110))
    tip = _polygon(scene, [(38, 71), (58, 71), (48, 84)])
    _paint(scene, tip, darken(tone(palette, "warm")[1], 0.14))


def build_bot(scene: Scene, palette: Palette) -> None:
    head = _rounded_rect(scene, 22, 25, 78, 74, 13)
    _extrude(scene, palette, head, role="violet", depth=7, shadow_offset=(0, 8), shadow_blur=9, gloss=58)
    antenna = union_masks(_rect(scene, 48, 18, 52, 30), _ellipse(scene, 50, 16, 4, 4))
    _paint(scene, antenna, lighten(tone(palette, "accent")[0], 0.06))
    side_l = _ellipse(scene, 22, 48, 2.6, 3.1)
    side_r = _ellipse(scene, 78, 48, 2.6, 3.1)
    _paint(scene, union_masks(side_l, side_r), lighten(tone(palette, "secondary")[0], 0.18))
    visor = _rounded_rect(scene, 32, 39, 68, 59, 8)
    _paint(scene, visor, darken(tone(palette, "neutral")[2], 0.18))
    eye_l = _rounded_rect(scene, 40, 44, 46, 53, 3)
    eye_r = _rounded_rect(scene, 54, 44, 60, 53, 3)
    _paint(scene, eye_l, tone(palette, "accent")[0])
    _paint(scene, eye_r, tone(palette, "accent")[0])
    mouth = _rounded_rect(scene, 43, 64, 57, 68, 2)
    _paint(scene, mouth, darken(tone(palette, "neutral")[2], 0.08))
    bolt_l = _ellipse(scene, 29, 43, 2, 2)
    bolt_r = _ellipse(scene, 71, 43, 2, 2)
    _paint(scene, bolt_l, lighten(tone(palette, "secondary")[0], 0.18))
    _paint(scene, bolt_r, lighten(tone(palette, "secondary")[0], 0.18))


def build_chat(scene: Scene, palette: Palette) -> None:
    bubble_back = _rounded_rect(scene, 24, 34, 58, 68, 11)
    tail_back = _polygon(scene, [(30, 62), (24, 74), (38, 66)])
    bubble_front = _rounded_rect(scene, 41, 22, 78, 56, 11)
    tail_front = _polygon(scene, [(66, 54), (74, 66), (61, 58)])
    _extrude(scene, palette, union_masks(bubble_back, tail_back), role="sky", depth=6, shadow_offset=(0, 7), shadow_blur=8, gloss=58)
    _extrude(scene, palette, union_masks(bubble_front, tail_front), role="accent", depth=6, shadow_offset=(0, 7), shadow_blur=8, gloss=58)
    dots = [
        _ellipse(scene, 48, 40, 2.2, 2.2),
        _ellipse(scene, 56, 40, 2.2, 2.2),
        _ellipse(scene, 64, 40, 2.2, 2.2),
    ]
    for dot in dots:
        _paint(scene, dot, darken(tone(palette, "neutral")[2], 0.04))


def build_devices(scene: Scene, palette: Palette) -> None:
    monitor = _rounded_rect(scene, 22, 28, 63, 62, 7)
    stand = union_masks(_rect(scene, 38, 61, 47, 70), _rounded_rect(scene, 32, 68, 53, 74, 3))
    phone = _rounded_rect(scene, 61, 34, 80, 74, 8)
    screen = _rounded_rect(scene, 28, 34, 57, 56, 5)
    phone_screen = _rounded_rect(scene, 65, 42, 76, 61, 4)
    _extrude(scene, palette, monitor, role="slate", depth=7, shadow_offset=(0, 8), shadow_blur=9, gloss=56)
    _paint(scene, stand, darken(tone(palette, "neutral")[2], 0.04))
    _extrude(scene, palette, phone, role="secondary", depth=6, shadow_offset=(0, 7), shadow_blur=8, gloss=56)
    _paint(scene, screen, darken(tone(palette, "neutral")[2], 0.14))
    _paint(scene, phone_screen, tone(palette, "accent")[0])


def build_globe(scene: Scene, palette: Palette) -> None:
    sphere = _ellipse(scene, 50, 48, 20, 20)
    _extrude(scene, palette, sphere, role="secondary", depth=7, shadow_offset=(0, 8), shadow_blur=9, gloss=54)
    equator = _clipped_line(scene, sphere, [(30, 48), (70, 48)], 3)
    meridian_left = _clipped_line(scene, sphere, [(42, 30), (42, 66)], 3)
    meridian_right = _clipped_line(scene, sphere, [(58, 30), (58, 66)], 3)
    _paint(scene, union_masks(equator, meridian_left, meridian_right), darken(tone(palette, "neutral")[2], 0.06))
    orbit = subtract_mask(_ellipse(scene, 50, 48, 27, 13), _ellipse(scene, 50, 48, 24, 10))
    _paint(scene, orbit, lighten(tone(palette, "accent")[0], 0.06))
    stand = union_masks(
        _rounded_rect(scene, 46, 70, 54, 78, 2),
        _rounded_rect(scene, 42, 76, 58, 80, 3),
    )
    _paint(scene, stand, darken(tone(palette, "neutral")[2], 0.08))


def build_history(scene: Scene, palette: Palette) -> None:
    clock = _ellipse(scene, 48, 48, 18, 18)
    _extrude(scene, palette, clock, role="warm", depth=6, shadow_offset=(0, 8), shadow_blur=8, gloss=56)
    rim = outline_mask(clock, scene.px(2))
    _paint(scene, rim, lighten(tone(palette, "warm")[0], 0.14))
    hand_hour = _clipped_line(scene, clock, [(48, 48), (48, 38)], 3.4)
    hand_minute = _clipped_line(scene, clock, [(48, 48), (60, 43)], 3.0)
    _paint(scene, union_masks(hand_hour, hand_minute), darken(tone(palette, "neutral")[2], 0.12))
    arc = subtract_mask(_ellipse(scene, 50, 48, 29, 29), _ellipse(scene, 50, 48, 25, 25))
    arc = intersect_mask(arc, _polygon(scene, [(74, 22), (78, 50), (66, 80), (46, 86), (28, 78), (24, 48), (34, 26)]))
    _paint(scene, arc, tone(palette, "accent")[0])
    arrow = _polygon(scene, [(69, 40), (80, 40), (75, 33)])
    _paint(scene, arrow, tone(palette, "accent")[1])


def build_system_logs(scene: Scene, palette: Palette) -> None:
    back = _rounded_rect(scene, 24, 28, 76, 75, 9)
    front = _rounded_rect(scene, 19, 22, 81, 70, 10)
    _extrude(scene, palette, back, role="neutral", depth=5, shadow_offset=(0, 6), shadow_blur=8, gloss=38)
    _extrude(scene, palette, front, role="secondary", depth=8, shadow_offset=(0, 9), shadow_blur=10, gloss=54)

    header = _rounded_rect(scene, 19, 22, 81, 36, 9)
    _paint(scene, header, tone(palette, "neutral")[0])
    for x, role in ((29, "danger"), (38, "warm"), (47, "accent")):
        _paint(scene, _ellipse(scene, x, 29, 2.2, 2.2), tone(palette, role)[0])

    screen = _rounded_rect(scene, 25, 37, 75, 65, 5)
    _paint(scene, screen, darken(tone(palette, "neutral")[2], 0.16))

    rows = [
        (31, 44, 57, "accent"),
        (31, 52, 64, "secondary"),
        (31, 60, 52, "warm"),
    ]
    for x1, y, x2, role in rows:
        _paint(scene, _ellipse(scene, 29, y, 1.8, 1.8), tone(palette, role)[0])
        _paint(scene, _line(scene, [(x1 + 5, y), (x2, y)], 2.1), lighten(tone(palette, role)[0], 0.08))

    tail = _rounded_rect(scene, 35, 69, 68, 77, 4)
    _extrude(scene, palette, tail, role="neutral", depth=4, shadow_offset=(0, 5), shadow_blur=6, gloss=36)
    spark = union_masks(
        _line(scene, [(67, 43), (73, 37), (79, 39)], 2.2),
        _ellipse(scene, 79, 39, 2.2, 2.2),
    )
    _paint(scene, spark, tone(palette, "warm")[0])


def _render_home_beetle(scene: Scene, palette: Palette) -> None:
    abdomen = _ellipse(scene, 50, 68, 8.6, 9.8)
    thorax = _ellipse(scene, 50, 59, 6.8, 5.0)
    head = _ellipse(scene, 50, 52, 4.8, 4.0)
    beetle = union_masks(abdomen, thorax, head)

    legs = union_masks(
        _line(scene, [(44, 60), (38, 57)], 1.3),
        _line(scene, [(56, 60), (62, 57)], 1.3),
        _line(scene, [(43, 67), (37, 67)], 1.3),
        _line(scene, [(57, 67), (63, 67)], 1.3),
        _line(scene, [(44, 74), (39, 77)], 1.3),
        _line(scene, [(56, 74), (61, 77)], 1.3),
    )
    antenna = union_masks(
        _line(scene, [(47, 49), (43, 46)], 1.1),
        _line(scene, [(53, 49), (57, 46)], 1.1),
        _ellipse(scene, 43, 46, 0.8, 0.8),
        _ellipse(scene, 57, 46, 0.8, 0.8),
    )
    _paint(scene, union_masks(legs, antenna), with_alpha(darken(tone(palette, "neutral")[2], 0.14), 150))

    _extrude(scene, palette, abdomen, role="primary", depth=4, shadow_offset=(0, 4), shadow_blur=5, gloss=48)
    _extrude(scene, palette, thorax, role="primary", depth=3, shadow_offset=(0, 3), shadow_blur=4, gloss=44)
    _extrude(scene, palette, head, role="primary", depth=3, shadow_offset=(0, 3), shadow_blur=4, gloss=42)

    seam = _clipped_line(scene, abdomen, [(50, 60), (50, 77)], 1.3)
    _paint(scene, seam, with_alpha(darken(tone(palette, "primary")[1], 0.12), 175))

    spots = union_masks(
        _ellipse(scene, 45.5, 65, 1.25, 1.25),
        _ellipse(scene, 54.5, 65, 1.25, 1.25),
        _ellipse(scene, 46, 72, 1.15, 1.15),
        _ellipse(scene, 54, 72, 1.15, 1.15),
    )
    _paint(scene, spots, tone(palette, "accent")[0])

    eyes = union_masks(_ellipse(scene, 48.3, 51.3, 0.7, 0.7), _ellipse(scene, 51.7, 51.3, 0.7, 0.7))
    _paint(scene, eyes, lighten(tone(palette, "neutral")[0], 0.18))
    scene.render_glow(beetle, blur=6, alpha=18)


def build_home(scene: Scene, palette: Palette) -> None:
    roof = _polygon(scene, [(22, 43), (50, 24), (78, 43), (69, 43), (50, 31), (31, 43)])
    roof_ridge = _polygon(scene, [(31, 43), (50, 32), (69, 43), (63, 43), (50, 38), (37, 43)])
    body = _rounded_rect(scene, 28, 43, 72, 82, 7)
    facade = _rounded_rect(scene, 33, 48, 67, 78, 5)
    chimney = _rounded_rect(scene, 62, 28, 68, 41, 2)
    threshold = _rounded_rect(scene, 36, 78, 64, 81, 2)

    _extrude(scene, palette, body, role="neutral", depth=7, shadow_offset=(0, 8), shadow_blur=9, gloss=54)
    _paint(scene, facade, lighten(tone(palette, "neutral")[0], 0.2))
    _extrude(scene, palette, roof, role="warm", depth=5, shadow_offset=(0, 6), shadow_blur=7, gloss=42)
    _paint(scene, roof_ridge, lighten(tone(palette, "warm")[0], 0.12))
    _paint(scene, chimney, darken(tone(palette, "warm")[1], 0.1))
    _paint(scene, threshold, darken(tone(palette, "neutral")[2], 0.04))

    _render_home_beetle(scene, palette)


def build_link(scene: Scene, palette: Palette) -> None:
    cable = _line(scene, [(39, 54), (49, 48), (61, 46)], 8)
    _extrude(scene, palette, cable, role="accent", depth=5, shadow_offset=(0, 5), shadow_blur=7, gloss=44)

    left = _rounded_rect(scene, 17, 39, 43, 69, 8)
    right = _rounded_rect(scene, 57, 31, 83, 61, 8)
    _extrude(scene, palette, left, role="sky", depth=7, shadow_offset=(0, 8), shadow_blur=9, gloss=56)
    _extrude(scene, palette, right, role="secondary", depth=7, shadow_offset=(0, 8), shadow_blur=9, gloss=56)

    left_socket = _rounded_rect(scene, 25, 49, 37, 58, 4)
    right_socket = _rounded_rect(scene, 63, 41, 75, 50, 4)
    _paint(scene, union_masks(left_socket, right_socket), darken(tone(palette, "neutral")[2], 0.16))

    pins = union_masks(
        _rounded_rect(scene, 36, 50, 47, 57, 3),
        _rounded_rect(scene, 53, 42, 64, 49, 3),
    )
    _paint(scene, pins, tone(palette, "accent")[0])

    status = union_masks(_ellipse(scene, 27, 44, 2.1, 2.1), _ellipse(scene, 74, 36, 2.1, 2.1))
    _paint(scene, status, lighten(tone(palette, "accent")[0], 0.1))
    shine = union_masks(_line(scene, [(22, 43), (39, 43)], 1.5), _line(scene, [(62, 35), (79, 35)], 1.5))
    _paint(scene, shine, with_alpha((255, 255, 255, 255), 130))


def build_power(scene: Scene, palette: Palette) -> None:
    button = _ellipse(scene, 50, 48, 21, 21)
    inner = _ellipse(scene, 50, 48, 15, 15)
    gap = _polygon(scene, [(47, 24), (53, 24), (53, 42), (47, 42)])
    symbol = subtract_mask(subtract_mask(button, inner), gap)
    _extrude(scene, palette, symbol, role="danger", depth=7, shadow_offset=(0, 8), shadow_blur=9, gloss=54)
    bar = _rounded_rect(scene, 48, 30, 52, 49, 2)
    _paint(scene, bar, tone(palette, "neutral")[2])


def build_puzzle(scene: Scene, palette: Palette) -> None:
    base = _polygon(
        scene,
        [
            (28, 38),
            (41, 38),
            (41, 33),
            (46, 28),
            (52, 28),
            (57, 33),
            (57, 38),
            (70, 38),
            (70, 51),
            (65, 51),
            (62, 56),
            (62, 62),
            (57, 67),
            (51, 67),
            (46, 62),
            (46, 57),
            (41, 57),
            (41, 70),
            (28, 70),
        ],
    )
    notch = _ellipse(scene, 69, 51, 4, 4)
    piece = subtract_mask(base, notch)
    _extrude(scene, palette, piece, role="accent", depth=7, shadow_offset=(0, 8), shadow_blur=9, gloss=56)
    nub = _ellipse(scene, 46, 28, 4.5, 4.5)
    _paint(scene, nub, lighten(tone(palette, "accent")[0], 0.08))


def build_skills(scene: Scene, palette: Palette) -> None:
    back = _rounded_rect(scene, 31, 23, 75, 69, 9)
    middle = _rounded_rect(scene, 25, 28, 80, 76, 10)
    front = _rounded_rect(scene, 18, 34, 72, 82, 11)
    _extrude(scene, palette, back, role="neutral", depth=5, shadow_offset=(0, 6), shadow_blur=8, gloss=38)
    _extrude(scene, palette, middle, role="secondary", depth=6, shadow_offset=(0, 7), shadow_blur=8, gloss=48)
    _extrude(scene, palette, front, role="primary", depth=8, shadow_offset=(0, 9), shadow_blur=10, gloss=56)

    header = _rounded_rect(scene, 18, 34, 72, 47, 10)
    _paint(scene, header, lighten(tone(palette, "primary")[0], 0.15))
    tab_a = _rounded_rect(scene, 30, 25, 44, 33, 4)
    tab_b = _rounded_rect(scene, 54, 30, 68, 38, 4)
    _paint(scene, union_masks(tab_a, tab_b), tone(palette, "accent")[0])

    skill_core = _polygon(
        scene,
        [
            (48, 48),
            (54, 58),
            (65, 60),
            (56, 66),
            (58, 77),
            (48, 70),
            (38, 77),
            (40, 66),
            (31, 60),
            (42, 58),
        ],
    )
    _extrude(scene, palette, skill_core, role="accent", depth=5, shadow_offset=(0, 5), shadow_blur=6, gloss=48)

    node_top = _ellipse(scene, 48, 58, 3.2, 3.2)
    node_left = _ellipse(scene, 39, 64, 2.6, 2.6)
    node_right = _ellipse(scene, 57, 64, 2.6, 2.6)
    links = union_masks(_line(scene, [(48, 58), (39, 64)], 1.8), _line(scene, [(48, 58), (57, 64)], 1.8))
    _paint(scene, links, with_alpha((255, 255, 255, 255), 150))
    _paint(scene, union_masks(node_top, node_left, node_right), lighten(tone(palette, "neutral")[0], 0.24))

    rows = union_masks(
        _rounded_rect(scene, 25, 52, 35, 55, 1.6),
        _rounded_rect(scene, 25, 71, 34, 74, 1.6),
        _rounded_rect(scene, 61, 52, 67, 55, 1.6),
        _rounded_rect(scene, 62, 71, 67, 74, 1.6),
    )
    _paint(scene, rows, darken(tone(palette, "primary")[1], 0.08))

    shine = _line(scene, [(25, 39), (60, 39)], 1.5)
    _paint(scene, shine, with_alpha((255, 255, 255, 255), 130))


def build_accounts(scene: Scene, palette: Palette) -> None:
    back_card = _rounded_rect(scene, 27, 23, 79, 70, 9)
    front_card = _rounded_rect(scene, 20, 30, 73, 79, 10)
    _extrude(scene, palette, back_card, role="neutral", depth=5, shadow_offset=(0, 6), shadow_blur=8, gloss=42)
    _extrude(scene, palette, front_card, role="sky", depth=8, shadow_offset=(0, 9), shadow_blur=10, gloss=56)

    header = _rounded_rect(scene, 20, 30, 73, 43, 10)
    _paint(scene, header, lighten(tone(palette, "sky")[0], 0.14))
    badge = union_masks(_ellipse(scene, 41, 51, 7.2, 7.2), _rounded_rect(scene, 29, 60, 53, 73, 7))
    _extrude(scene, palette, badge, role="primary", depth=5, shadow_offset=(0, 5), shadow_blur=7, gloss=48)

    face_cut = _ellipse(scene, 41, 51, 3.2, 3.2)
    neck_cut = _rounded_rect(scene, 38, 57, 44, 63, 2)
    shoulder_cut = _rounded_rect(scene, 33, 64, 49, 69, 3)
    _paint(scene, union_masks(face_cut, neck_cut, shoulder_cut), lighten(tone(palette, "neutral")[0], 0.22))

    line_a = _rounded_rect(scene, 56, 49, 70, 53, 2)
    line_b = _rounded_rect(scene, 56, 59, 68, 63, 2)
    _paint(scene, union_masks(line_a, line_b), darken(tone(palette, "neutral")[2], 0.06))

    key_ring = subtract_mask(_ellipse(scene, 67, 67, 6.3, 6.3), _ellipse(scene, 67, 67, 3.4, 3.4))
    key_shaft = _line(scene, [(70, 67), (82, 67)], 4.2)
    key_teeth = union_masks(_rect(scene, 78, 67, 81, 74), _rect(scene, 82, 67, 85, 71))
    _extrude(scene, palette, union_masks(key_ring, key_shaft, key_teeth), role="warm", depth=5, shadow_offset=(0, 5), shadow_blur=6, gloss=44)

    status_dot = _ellipse(scene, 29, 37, 2.2, 2.2)
    _paint(scene, status_dot, tone(palette, "accent")[0])
    shine = _line(scene, [(28, 35), (64, 35)], 1.5)
    _paint(scene, shine, with_alpha((255, 255, 255, 255), 130))


def build_safe(scene: Scene, palette: Palette) -> None:
    shell = _rounded_rect(scene, 26, 30, 74, 74, 9)
    _extrude(scene, palette, shell, role="slate", depth=8, shadow_offset=(0, 9), shadow_blur=10, gloss=54)
    door = _ellipse(scene, 50, 52, 18, 18)
    _paint(scene, door, darken(tone(palette, "neutral")[2], 0.1))
    ring = outline_mask(door, scene.px(3))
    _paint(scene, ring, lighten(tone(palette, "neutral")[0], 0.12))
    hub = _ellipse(scene, 50, 52, 5, 5)
    _paint(scene, hub, tone(palette, "accent")[0])
    spokes = [
        _rect(scene, 49, 40, 51, 48),
        _rect(scene, 49, 56, 51, 64),
        _rect(scene, 40, 51, 48, 53),
        _rect(scene, 52, 51, 60, 53),
    ]
    for spoke in spokes:
        _paint(scene, spoke, lighten(tone(palette, "neutral")[0], 0.06))


def build_settings(scene: Scene, palette: Palette) -> None:
    teeth = [
        _rounded_rect(scene, 46, 24, 54, 34, 2),
        _rounded_rect(scene, 46, 66, 54, 76, 2),
        _rounded_rect(scene, 24, 46, 34, 54, 2),
        _rounded_rect(scene, 66, 46, 76, 54, 2),
        _rounded_rect(scene, 33, 33, 40, 40, 2),
        _rounded_rect(scene, 60, 33, 67, 40, 2),
        _rounded_rect(scene, 33, 60, 40, 67, 2),
        _rounded_rect(scene, 60, 60, 67, 67, 2),
    ]
    gear = union_masks(*teeth, _ellipse(scene, 50, 50, 17, 17))
    gear = subtract_mask(gear, _ellipse(scene, 50, 50, 6, 6))
    _extrude(scene, palette, gear, role="violet", depth=7, shadow_offset=(0, 8), shadow_blur=9, gloss=56)
    hub = _ellipse(scene, 50, 50, 5, 5)
    _paint(scene, hub, tone(palette, "accent")[0])


def build_strategy(scene: Scene, palette: Palette) -> None:
    board = _rounded_rect(scene, 24, 28, 76, 76, 8)
    _extrude(scene, palette, board, role="coral", depth=7, shadow_offset=(0, 8), shadow_blur=9, gloss=54)
    path = _clipped_line(scene, board, [(34, 64), (46, 54), (56, 56), (66, 40)], 3.2)
    _paint(scene, path, darken(tone(palette, "neutral")[2], 0.06))
    nodes = union_masks(_ellipse(scene, 34, 64, 3, 3), _ellipse(scene, 46, 54, 3, 3), _ellipse(scene, 56, 56, 3, 3), _ellipse(scene, 66, 40, 3, 3))
    _paint(scene, nodes, tone(palette, "accent")[0])
    flag = _polygon(scene, [(64, 34), (72, 34), (72, 42), (64, 38)])
    mast = _rect(scene, 63, 34, 65, 48)
    _paint(scene, union_masks(flag, mast), lighten(tone(palette, "accent")[0], 0.12))


def build_theme(scene: Scene, palette: Palette) -> None:
    dial = _ellipse(scene, 50, 48, 19, 19)
    _extrude(scene, palette, dial, role="warm", depth=7, shadow_offset=(0, 8), shadow_blur=9, gloss=56)
    split = _rect(scene, 50, 30, 70, 66)
    _paint(scene, split, tone(palette, "accent")[0])
    moon = subtract_mask(_ellipse(scene, 44, 48, 10, 10), _ellipse(scene, 47, 46, 8, 8))
    _paint(scene, moon, darken(tone(palette, "neutral")[2], 0.06))
    sun = _ellipse(scene, 58, 48, 5, 5)
    _paint(scene, sun, tone(palette, "danger")[0])
    spark = union_masks(_rect(scene, 57, 34, 59, 40), _rect(scene, 54, 37, 62, 39))
    _paint(scene, spark, lighten(tone(palette, "accent")[0], 0.12))


def build_tools(scene: Scene, palette: Palette) -> None:
    handle_outer = _rounded_rect(scene, 35, 24, 65, 43, 7)
    handle_inner = _rounded_rect(scene, 42, 31, 58, 43, 4)
    handle = subtract_mask(handle_outer, handle_inner)
    _extrude(scene, palette, handle, role="neutral", depth=5, shadow_offset=(0, 5), shadow_blur=7, gloss=38)

    case = _rounded_rect(scene, 17, 39, 83, 80, 10)
    _extrude(scene, palette, case, role="slate", depth=9, shadow_offset=(0, 9), shadow_blur=11, gloss=54)

    lid = _rounded_rect(scene, 20, 34, 80, 53, 8)
    _extrude(scene, palette, lid, role="secondary", depth=6, shadow_offset=(0, 6), shadow_blur=8, gloss=48)

    tray = _rounded_rect(scene, 23, 54, 77, 75, 6)
    _paint(scene, tray, darken(tone(palette, "neutral")[2], 0.06))

    latch = _rounded_rect(scene, 45, 48, 55, 59, 3)
    _extrude(scene, palette, latch, role="warm", depth=4, shadow_offset=(0, 4), shadow_blur=5, gloss=38)

    wrench_head = subtract_mask(_ellipse(scene, 35, 60, 8, 8), _ellipse(scene, 35, 60, 4.2, 4.2))
    wrench_head = subtract_mask(wrench_head, _polygon(scene, [(30, 55), (37, 62), (33, 66), (26, 59)]))
    wrench_handle = union_masks(
        _line(scene, [(41, 61), (64, 70)], 6.2),
        _ellipse(scene, 64, 70, 3.2, 3.2),
    )
    _paint(scene, union_masks(wrench_head, wrench_handle), lighten(tone(palette, "neutral")[0], 0.18))

    driver = union_masks(
        _line(scene, [(61, 45), (38, 69)], 5.2),
        _polygon(scene, [(34, 72), (39, 67), (43, 72), (38, 77)]),
        _rounded_rect(scene, 59, 41, 70, 49, 3),
    )
    _paint(scene, driver, tone(palette, "accent")[0])

    shine = _line(scene, [(25, 42), (72, 42)], 1.6)
    _paint(scene, shine, with_alpha((255, 255, 255, 255), 120))


def build_unsaved_changes(scene: Scene, palette: Palette) -> None:
    page = _polygon(scene, [(27, 22), (61, 22), (74, 35), (74, 80), (27, 80)])
    fold = _polygon(scene, [(61, 22), (74, 35), (61, 35)])
    _extrude(scene, palette, page, role="neutral", depth=7, shadow_offset=(0, 8), shadow_blur=9, gloss=52)
    _paint(scene, fold, darken(tone(palette, "neutral")[0], 0.08))

    title = _rounded_rect(scene, 35, 39, 59, 43, 2)
    line_a = _rounded_rect(scene, 35, 51, 63, 55, 2)
    line_b = _rounded_rect(scene, 35, 62, 55, 66, 2)
    _paint(scene, union_masks(title, line_a, line_b), darken(tone(palette, "neutral")[2], 0.08))

    pencil_body = _line(scene, [(39, 73), (65, 52)], 8)
    pencil_wood = _polygon(scene, [(63, 50), (72, 44), (70, 52)])
    pencil_tip = _polygon(scene, [(70, 45), (75, 42), (72, 49)])
    pencil_eraser = _rounded_rect(scene, 33, 70, 43, 79, 3)
    _extrude(scene, palette, pencil_body, role="warm", depth=5, shadow_offset=(0, 5), shadow_blur=6, gloss=46)
    _paint(scene, pencil_wood, lighten(tone(palette, "warm")[0], 0.08))
    _paint(scene, pencil_tip, darken(tone(palette, "neutral")[2], 0.12))
    _paint(scene, pencil_eraser, tone(palette, "danger")[0])

    pending = _ellipse(scene, 65, 31, 7, 7)
    _extrude(scene, palette, pending, role="accent", depth=4, shadow_offset=(0, 4), shadow_blur=5, gloss=42)
    _paint(scene, _rect(scene, 64, 26, 66, 32), lighten(tone(palette, "neutral")[0], 0.2))
    _paint(scene, _ellipse(scene, 65, 35, 1.4, 1.4), lighten(tone(palette, "neutral")[0], 0.2))


ICON_DEFINITIONS: list[IconSpec] = [
    icon("accounts_3d.png", "sky", build_accounts),
    icon("bookmark_3d.png", "amber", build_bookmark),
    icon("bot_3d.png", "violet", build_bot),
    icon("chat_3d.png", "sky", build_chat),
    icon("devices_3d.png", "slate", build_devices),
    icon("globe_3d.png", "teal", build_globe),
    icon("history_3d.png", "coral", build_history),
    icon("home_3d.png", "violet", build_home),
    icon("link_3d.png", "sky", build_link),
    icon("power_3d.png", "crimson", build_power),
    icon("puzzle_3d.png", "mint", build_puzzle),
    icon("safe_3d.png", "slate", build_safe),
    icon("settings_3d.png", "violet", build_settings),
    icon("skills_3d.png", "mint", build_skills),
    icon("strategy_3d.png", "coral", build_strategy),
    icon("system_logs_3d.png", "slate", build_system_logs),
    icon("theme_3d.png", "amber", build_theme),
    icon("tools_3d.png", "slate", build_tools),
    icon("unsaved_changes_3d.png", "crimson", build_unsaved_changes),
]
