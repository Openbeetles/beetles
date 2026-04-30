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
    "i2c_sensor_config_3d.png",
    "language_3d.png",
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
    page = _rounded_rect(scene, 22, 21, 75, 82, 9)
    _extrude(scene, palette, page, role="neutral", depth=9, shadow_offset=(0, 9), shadow_blur=10, gloss=58)

    spine = _rounded_rect(scene, 22, 21, 34, 82, 8)
    _paint(scene, spine, lighten(tone(palette, "secondary")[0], 0.08))
    _paint(scene, _rect(scene, 33, 27, 36, 76), with_alpha(darken(tone(palette, "neutral")[1], 0.08), 95))

    ribbon_shadow = _polygon(scene, [(55, 20), (75, 20), (75, 73), (65, 65), (55, 73)])
    _paint(scene, shift_mask(ribbon_shadow, scene.scalar(2), scene.scalar(3)), with_alpha(palette.shadow, 72))
    ribbon = _polygon(scene, [(54, 18), (76, 18), (76, 72), (65, 63), (54, 72)])
    _extrude(scene, palette, ribbon, role="warm", depth=7, shadow_offset=(0, 6), shadow_blur=7, gloss=66)

    notch = _polygon(scene, [(58, 23), (72, 23), (72, 61), (65, 55), (58, 61)])
    _paint(scene, notch, lighten(tone(palette, "warm")[0], 0.1))
    _paint(scene, _rect(scene, 61, 26, 64, 56), with_alpha((255, 255, 255, 255), 95))

    for y, right in ((38, 50), (49, 47), (60, 50)):
        _paint(scene, _line(scene, [(39, y), (right, y)], 2.0), with_alpha(darken(tone(palette, "neutral")[1], 0.18), 130))

    marker = _ellipse(scene, 66, 31, 3.3, 3.3)
    _extrude(scene, palette, marker, role="accent", depth=4, shadow_offset=(0, 4), shadow_blur=5, gloss=42)


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
    sphere = _ellipse(scene, 50, 50, 24, 24)
    _extrude(scene, palette, sphere, role="secondary", depth=8, shadow_offset=(0, 8), shadow_blur=10, gloss=58)

    meridian_left = intersect_mask(subtract_mask(_ellipse(scene, 50, 50, 13, 24), _ellipse(scene, 50, 50, 10.8, 22)), sphere)
    meridian_right = intersect_mask(subtract_mask(_ellipse(scene, 50, 50, 24, 13), _ellipse(scene, 50, 50, 21.8, 10.8)), sphere)
    equator = _clipped_line(scene, sphere, [(28, 50), (72, 50)], 1.7)
    latitude_top = _clipped_line(scene, sphere, [(33, 41), (67, 41)], 1.2)
    latitude_bottom = _clipped_line(scene, sphere, [(33, 59), (67, 59)], 1.2)
    _paint(scene, union_masks(meridian_left, meridian_right, equator, latitude_top, latitude_bottom), with_alpha((255, 255, 255, 255), 92))

    accent_node = _ellipse(scene, 62, 39, 3.0, 3.0)
    _paint(scene, accent_node, tone(palette, "accent")[0])

    shine = intersect_mask(_ellipse(scene, 41, 38, 6, 3), sphere)
    _paint(scene, shine, with_alpha((255, 255, 255, 255), 130))


def build_language(scene: Scene, palette: Palette) -> None:
    back = _rounded_rect(scene, 45, 20, 83, 61, 8)
    back_tail = _polygon(scene, [(75, 57), (83, 64), (73, 63)])
    _extrude(scene, palette, union_masks(back, back_tail), role="warm", depth=7, shadow_offset=(0, 7), shadow_blur=8, gloss=58)

    front = _rounded_rect(scene, 17, 35, 60, 79, 10)
    front_tail = _polygon(scene, [(27, 74), (17, 84), (36, 78)])
    _extrude(scene, palette, union_masks(front, front_tail), role="secondary", depth=8, shadow_offset=(0, 8), shadow_blur=9, gloss=60)

    wen = union_masks(
        _line(scene, [(61, 32), (78, 32)], 2.2),
        _line(scene, [(69, 33), (69, 43)], 2.1),
        _line(scene, [(62, 49), (69, 43), (78, 50)], 2.1),
    )
    _paint(scene, wen, with_alpha(darken(tone(palette, "warm")[1], 0.24), 150))

    letter_a = union_masks(
        _line(scene, [(29, 68), (40, 46)], 4.0),
        _line(scene, [(40, 46), (51, 68)], 4.0),
        _line(scene, [(33, 59), (47, 59)], 3.2),
    )
    _paint(scene, letter_a, with_alpha((255, 255, 255, 255), 235))

    swap = union_masks(
        _line(scene, [(55, 70), (70, 70), (75, 64)], 2.5),
        _line(scene, [(72, 64), (75, 64), (75, 68)], 2.3),
    )
    _paint(scene, swap, lighten(tone(palette, "accent")[0], 0.08))


def build_history(scene: Scene, palette: Palette) -> None:
    clock = _ellipse(scene, 50, 50, 19, 19)
    _extrude(scene, palette, clock, role="warm", depth=6, shadow_offset=(0, 8), shadow_blur=8, gloss=56)
    rim = outline_mask(clock, scene.px(2))
    _paint(scene, rim, lighten(tone(palette, "warm")[0], 0.14))
    hand_hour = _clipped_line(scene, clock, [(50, 50), (50, 39)], 3.3)
    hand_minute = _clipped_line(scene, clock, [(50, 50), (62, 45)], 3.0)
    _paint(scene, union_masks(hand_hour, hand_minute), darken(tone(palette, "neutral")[2], 0.12))

    arc = union_masks(
        scene.arc_band(50, 50, 31, 27, 142, 360),
        scene.arc_band(50, 50, 31, 27, 0, 42),
    )
    _paint(scene, arc, tone(palette, "accent")[0])
    arrow = _polygon(scene, [(76, 36), (88, 42), (77, 50)])
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
    badge = _rounded_rect(scene, 38, 53, 62, 78, 8)
    _extrude(scene, palette, badge, role="primary", depth=4, shadow_offset=(0, 4), shadow_blur=5, gloss=44)

    head = _ellipse(scene, 50, 58, 4.2, 3.5)
    shell_left = _ellipse(scene, 46.5, 67, 5.5, 8.2)
    shell_right = _ellipse(scene, 53.5, 67, 5.5, 8.2)
    body = union_masks(head, shell_left, shell_right)
    _paint(scene, body, lighten(tone(palette, "primary")[0], 0.12))

    center = _clipped_line(scene, badge, [(50, 60), (50, 75)], 1.3)
    neck = _clipped_line(scene, badge, [(45, 61), (55, 61)], 1.1)
    _paint(scene, union_masks(center, neck), with_alpha(darken(tone(palette, "primary")[1], 0.2), 150))

    antenna = union_masks(
        _line(scene, [(47.5, 55), (44.5, 52.5)], 0.9),
        _line(scene, [(52.5, 55), (55.5, 52.5)], 0.9),
        _ellipse(scene, 44.5, 52.5, 0.7, 0.7),
        _ellipse(scene, 55.5, 52.5, 0.7, 0.7),
    )
    _paint(scene, antenna, with_alpha(lighten(tone(palette, "primary")[0], 0.18), 185))

    spots = union_masks(
        _ellipse(scene, 45.5, 66, 1.05, 1.05),
        _ellipse(scene, 54.5, 66, 1.05, 1.05),
        _ellipse(scene, 46.5, 72, 0.95, 0.95),
        _ellipse(scene, 53.5, 72, 0.95, 0.95),
    )
    _paint(scene, spots, tone(palette, "accent")[0])

    shine = _clipped_line(scene, badge, [(42, 57), (55, 57)], 1.0)
    _paint(scene, shine, with_alpha((255, 255, 255, 255), 115))


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
    body = _rounded_rect(scene, 25, 34, 76, 78, 7)
    top_tab = union_masks(_rounded_rect(scene, 42, 27, 58, 40, 5), _ellipse(scene, 50, 27, 9.2, 9.2))
    left_tab = _ellipse(scene, 25, 56, 8.5, 8.5)
    piece = union_masks(body, top_tab, left_tab)

    right_socket = _ellipse(scene, 76, 56, 8.0, 8.0)
    bottom_socket = _ellipse(scene, 50, 78, 7.6, 7.6)
    piece = subtract_mask(subtract_mask(piece, right_socket), bottom_socket)
    _extrude(scene, palette, piece, role="accent", depth=8, shadow_offset=(0, 9), shadow_blur=10, gloss=58)

    top_gloss = intersect_mask(_ellipse(scene, 43, 39, 17, 6), piece)
    _paint(scene, top_gloss, with_alpha((255, 255, 255, 255), 92))
    socket_shadow = intersect_mask(_ellipse(scene, 74, 56, 9.2, 9.2), piece)
    _paint(scene, socket_shadow, with_alpha(darken(tone(palette, "accent")[1], 0.18), 95))
    foot_shadow = intersect_mask(_ellipse(scene, 50, 76, 8.8, 5.2), piece)
    _paint(scene, foot_shadow, with_alpha(darken(tone(palette, "accent")[1], 0.14), 70))


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

    grooves = union_masks(
        _clipped_line(scene, gear, [(50, 32), (50, 42)], 1.6),
        _clipped_line(scene, gear, [(50, 58), (50, 68)], 1.6),
        _clipped_line(scene, gear, [(32, 50), (42, 50)], 1.6),
        _clipped_line(scene, gear, [(58, 50), (68, 50)], 1.6),
    )
    _paint(scene, grooves, with_alpha(darken(tone(palette, "primary")[1], 0.2), 92))

    face_gloss = intersect_mask(_ellipse(scene, 43, 37, 17, 6), gear)
    _paint(scene, face_gloss, with_alpha((255, 255, 255, 255), 78))

    recess = subtract_mask(_ellipse(scene, 50, 50, 10, 10), _ellipse(scene, 50, 50, 6.1, 6.1))
    _paint(scene, recess, with_alpha(darken(tone(palette, "primary")[1], 0.22), 170))
    inner_lip = subtract_mask(_ellipse(scene, 50, 50, 7.2, 7.2), _ellipse(scene, 50, 50, 5.7, 5.7))
    _paint(scene, inner_lip, with_alpha(lighten(tone(palette, "primary")[0], 0.08), 135))

    hub = _ellipse(scene, 50, 50, 5.8, 5.8)
    _extrude(scene, palette, hub, role="accent", depth=4, shadow_offset=(0, 4), shadow_blur=5, gloss=48)
    hub_glint = _ellipse(scene, 47, 46, 1.6, 1.2)
    _paint(scene, hub_glint, with_alpha((255, 255, 255, 255), 128))


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
    tile = _rounded_rect(scene, 20, 22, 80, 78, 14)
    _extrude(scene, palette, tile, role="neutral", depth=8, shadow_offset=(0, 9), shadow_blur=10, gloss=52)

    inset = _rounded_rect(scene, 25, 27, 75, 72, 11)
    day_side = intersect_mask(inset, _rect(scene, 25, 27, 51, 72))
    night_side = intersect_mask(inset, _rect(scene, 49, 27, 75, 72))
    _paint(scene, day_side, lighten(tone(palette, "warm")[0], 0.07))
    _paint(scene, night_side, darken(tone(palette, "accent")[1], 0.1))

    seam = _rounded_rect(scene, 47.5, 30, 52.5, 69, 2.5)
    _paint(scene, intersect_mask(seam, inset), with_alpha(darken(tone(palette, "neutral")[2], 0.1), 70))

    sun_core = _ellipse(scene, 38, 49, 6.0, 6.0)
    sun_rays = union_masks(
        _line(scene, [(38, 36), (38, 41)], 2.2),
        _line(scene, [(38, 57), (38, 62)], 2.2),
        _line(scene, [(27, 49), (32, 49)], 2.2),
        _line(scene, [(44, 49), (49, 49)], 2.2),
        _line(scene, [(30, 40), (33, 43)], 2.0),
        _line(scene, [(43, 55), (46, 58)], 2.0),
        _line(scene, [(30, 58), (33, 55)], 2.0),
        _line(scene, [(43, 43), (46, 40)], 2.0),
    )
    _paint(scene, sun_rays, lighten(tone(palette, "primary")[0], 0.12))
    _paint(scene, sun_core, lighten(tone(palette, "primary")[0], 0.18))

    moon = subtract_mask(_ellipse(scene, 62, 49, 9.0, 9.0), _ellipse(scene, 66, 45, 8.3, 8.3))
    _paint(scene, moon, lighten(tone(palette, "neutral")[0], 0.18))
    _paint(scene, _ellipse(scene, 70, 35, 2.0, 2.0), lighten(tone(palette, "neutral")[0], 0.2))
    _paint(scene, _ellipse(scene, 69, 63, 1.6, 1.6), with_alpha((255, 255, 255, 255), 150))

    shine = _clipped_line(scene, tile, [(28, 27), (72, 27)], 1.5)
    _paint(scene, shine, with_alpha((255, 255, 255, 255), 118))


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


def build_i2c_sensor_config(scene: Scene, palette: Palette) -> None:
    board = _rounded_rect(scene, 18, 30, 82, 70, 9)
    _extrude(scene, palette, board, role="primary", depth=8, shadow_offset=(0, 9), shadow_blur=10, gloss=58)

    for x in (27, 38, 49, 60, 71):
        pin = _rounded_rect(scene, x - 2.4, 67, x + 2.4, 81, 2.0)
        _extrude(scene, palette, pin, role="warm", depth=4, shadow_offset=(0, 4), shadow_blur=5, gloss=45)

    chip = _rounded_rect(scene, 34, 39, 66, 60, 5)
    _paint(scene, shift_mask(chip, scene.scalar(2), scene.scalar(3)), with_alpha(palette.shadow, 70))
    _paint(scene, chip, darken(tone(palette, "neutral")[2], 0.04))
    _paint(scene, _rounded_rect(scene, 38, 43, 62, 47, 2), with_alpha((255, 255, 255, 255), 48))

    for y in (43, 49, 55):
        trace_l = _line(scene, [(22, y), (34, y)], 1.8)
        trace_r = _line(scene, [(66, y), (78, y)], 1.8)
        _paint(scene, union_masks(trace_l, trace_r), with_alpha(lighten(tone(palette, "accent")[0], 0.08), 170))

    droplet = _polygon(scene, [(74, 30), (84, 46), (75, 57), (66, 46)])
    droplet_round = _ellipse(scene, 75, 47, 8, 9)
    _extrude(scene, palette, union_masks(droplet, droplet_round), role="secondary", depth=5, shadow_offset=(0, 6), shadow_blur=7, gloss=62)
    _paint(scene, _ellipse(scene, 72, 43, 2.5, 3.0), with_alpha((255, 255, 255, 255), 120))

    bulb = _ellipse(scene, 25, 57, 5.5, 5.5)
    stem = _rounded_rect(scene, 23, 35, 27, 58, 2)
    _extrude(scene, palette, union_masks(bulb, stem), role="warm", depth=5, shadow_offset=(0, 6), shadow_blur=7, gloss=52)
    _paint(scene, _line(scene, [(31, 41), (37, 41)], 1.4), with_alpha((255, 255, 255, 255), 140))
    _paint(scene, _line(scene, [(31, 49), (36, 49)], 1.4), with_alpha((255, 255, 255, 255), 110))

    for cx, cy in ((24, 35), (76, 35), (24, 65), (76, 65)):
        _paint(scene, _ellipse(scene, cx, cy, 2.4, 2.4), lighten(tone(palette, "neutral")[0], 0.16))


ICON_DEFINITIONS: list[IconSpec] = [
    icon("accounts_3d.png", "sky", build_accounts),
    icon("bookmark_3d.png", "amber", build_bookmark),
    icon("bot_3d.png", "violet", build_bot),
    icon("chat_3d.png", "sky", build_chat),
    icon("devices_3d.png", "slate", build_devices),
    icon("globe_3d.png", "teal", build_globe),
    icon("history_3d.png", "coral", build_history),
    icon("home_3d.png", "violet", build_home),
    icon("i2c_sensor_config_3d.png", "mint", build_i2c_sensor_config),
    icon("language_3d.png", "sky", build_language),
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
