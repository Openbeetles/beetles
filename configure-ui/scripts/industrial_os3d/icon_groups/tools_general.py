from __future__ import annotations

from industrial_os3d.icon_groups.common import (
    IconSpec,
    Scene,
    blank_mask,
    darken,
    icon,
    intersect_mask,
    lighten,
    outline_mask,
    subtract_mask,
    tone,
    union_masks,
    with_alpha,
)

ICON_GROUP = "tools_general"

OWNED_ICONS = (
    "file_write_3d.png",
    "search_3d.png",
    "tool_cron_3d.png",
    "tool_doc_extract_3d.png",
    "tool_doc_read_3d.png",
    "tool_doc_search_3d.png",
    "tool_documents_3d.png",
    "tool_edit_3d.png",
    "tool_http_3d.png",
    "tool_message_3d.png",
    "tool_pdf_3d.png",
    "tool_remind_list_3d.png",
    "tool_task_3d.png",
    "tool_web_fetch_3d.png",
    "tool_write_3d.png",
)


def _solid(scene: Scene, mask, color) -> None:
    scene._paste_fill(mask, color)


def _flat_role(scene: Scene, palette, mask, role: str, alpha: int | None = None) -> None:
    top, bottom, _ = tone(palette, role)
    scene.render_flat(mask, top, bottom, alpha=alpha)


def _extrude_role(
    scene: Scene,
    palette,
    mask,
    role: str,
    *,
    depth: int = 10,
    shadow_offset: tuple[int, int] = (0, 10),
    shadow_blur: int = 10,
    gloss: int = 66,
) -> None:
    top, bottom, side = tone(palette, role)
    scene.render_extruded(
        mask,
        top,
        bottom,
        side,
        depth=depth,
        shadow_offset=shadow_offset,
        shadow_blur=shadow_blur,
        gloss=gloss,
    )


def _folded_page(scene: Scene, x1: float, y1: float, x2: float, y2: float, radius: float, fold: float):
    body = scene.rounded_rect(x1, y1, x2, y2, radius)
    fold_mask = scene.polygon([(x2 - fold, y1), (x2, y1), (x2, y1 + fold)])
    body = subtract_mask(body, fold_mask)
    seam = scene.line([(x2 - fold, y1 + 0.4), (x2 - 0.4, y1 + fold)], 1.3)
    return body, fold_mask, seam


def build_search(scene: Scene, palette) -> None:
    lens = union_masks(scene.circle(52, 41, 23), scene.circle(52, 41, 19))
    _extrude_role(scene, palette, lens, "primary", depth=12, shadow_blur=14, gloss=78)
    _solid(scene, scene.circle(59, 34, 6), with_alpha((255, 255, 255, 255), 130))

    handle = union_masks(
        scene.line([(29, 72), (43, 58)], 14),
        scene.circle(29, 72, 7),
        scene.circle(43, 58, 7),
    )
    _extrude_role(scene, palette, handle, "warm", depth=8, shadow_offset=(1, 8), shadow_blur=11, gloss=52)


def build_cron(scene: Scene, palette) -> None:
    panel = scene.rounded_rect(19, 25, 81, 79, 13)
    _extrude_role(scene, palette, panel, "neutral", depth=9, shadow_offset=(0, 9), shadow_blur=12, gloss=56)

    header = scene.rounded_rect(19, 25, 81, 42, 13)
    _flat_role(scene, palette, header, "warm")

    binder_left = scene.rounded_rect(32, 16, 39, 33, 3)
    binder_right = scene.rounded_rect(61, 16, 68, 33, 3)
    _extrude_role(scene, palette, binder_left, "accent", depth=4, shadow_offset=(0, 4), shadow_blur=6, gloss=48)
    _extrude_role(scene, palette, binder_right, "accent", depth=4, shadow_offset=(0, 4), shadow_blur=6, gloss=48)

    face = scene.circle(50, 56, 21)
    _extrude_role(scene, palette, face, "primary", depth=8, shadow_offset=(0, 8), shadow_blur=10, gloss=66)
    inner = scene.circle(50, 56, 14)
    _flat_role(scene, palette, inner, "neutral", alpha=245)

    recurrence = union_masks(
        scene.arc_band(50, 56, 29, 25, 205, 360),
        scene.arc_band(50, 56, 29, 25, 0, 78),
    )
    _flat_role(scene, palette, recurrence, "secondary", alpha=235)
    arrow = scene.polygon([(73, 40), (84, 41), (78, 31)])
    _extrude_role(scene, palette, arrow, "secondary", depth=5, shadow_offset=(0, 5), shadow_blur=6, gloss=50)

    hour = scene.line([(50, 56), (50, 45)], 3.0)
    minute = scene.line([(50, 56), (61, 61)], 2.6)
    _solid(scene, union_masks(hour, minute), darken(tone(palette, "neutral")[2], 0.1))
    _flat_role(scene, palette, scene.circle(50, 56, 2.8), "danger")

    tick_top = scene.line([(50, 40), (50, 43)], 1.5)
    tick_left = scene.line([(34, 56), (37, 56)], 1.5)
    tick_right = scene.line([(63, 56), (66, 56)], 1.5)
    for tick in (tick_top, tick_left, tick_right):
        _solid(scene, tick, with_alpha((255, 255, 255, 255), 175))


def build_doc_extract(scene: Scene, palette) -> None:
    tray = scene.rounded_rect(29, 49, 71, 76, 10)
    _extrude_role(scene, palette, tray, "secondary", depth=8, shadow_offset=(0, 8), shadow_blur=11, gloss=54)
    tray_rim = outline_mask(tray, scene.px(2))
    _solid(scene, tray_rim, with_alpha((255, 255, 255, 255), 100))

    body, fold, seam = _folded_page(scene, 24, 26, 76, 63, 10, 10)
    _extrude_role(scene, palette, body, "neutral", depth=6, shadow_offset=(0, 7), shadow_blur=10, gloss=62)
    _flat_role(scene, palette, fold, "primary")
    _solid(scene, seam, with_alpha((255, 255, 255, 255), 150))

    arrow = union_masks(
        scene.rounded_rect(45, 27, 55, 57, 4),
        scene.polygon([(38, 40), (62, 40), (50, 22)]),
    )
    _extrude_role(scene, palette, arrow, "danger", depth=6, shadow_offset=(0, 7), shadow_blur=9, gloss=68)


def build_doc_read(scene: Scene, palette) -> None:
    base = scene.rounded_rect(18, 69, 82, 80, 6)
    _extrude_role(scene, palette, base, "secondary", depth=6, shadow_offset=(0, 7), shadow_blur=10, gloss=48)

    left_page = scene.rounded_rect(19, 27, 49, 69, 8)
    right_page = scene.rounded_rect(51, 27, 81, 69, 8)
    gutter = scene.rounded_rect(48, 33, 52, 72, 2)
    pages = subtract_mask(union_masks(left_page, right_page), gutter)
    _extrude_role(scene, palette, pages, "neutral", depth=7, shadow_offset=(0, 8), shadow_blur=10, gloss=58)

    center_fold = scene.line([(50, 31), (50, 69)], 2)
    _solid(scene, center_fold, with_alpha((185, 188, 204, 255), 170))

    left_curve = scene.line([(27, 58), (36, 64), (45, 61)], 2.4)
    right_curve = scene.line([(55, 61), (64, 64), (73, 58)], 2.4)
    _solid(scene, left_curve, with_alpha((255, 255, 255, 255), 100))
    _solid(scene, right_curve, with_alpha((255, 255, 255, 255), 100))


def build_doc_search(scene: Scene, palette) -> None:
    base = scene.rounded_rect(19, 74, 81, 84, 5)
    _extrude_role(scene, palette, base, "secondary", depth=5, shadow_offset=(0, 6), shadow_blur=8, gloss=42)

    arm = union_masks(
        scene.line([(30, 64), (47, 50)], 10),
        scene.circle(30, 64, 5),
        scene.circle(47, 50, 5),
    )
    _extrude_role(scene, palette, arm, "accent", depth=6, shadow_offset=(0, 7), shadow_blur=9, gloss=58)

    body = union_masks(
        scene.circle(58, 33, 12),
        scene.rounded_rect(50, 31, 67, 47, 8),
    )
    _extrude_role(scene, palette, body, "neutral", depth=6, shadow_offset=(1, 7), shadow_blur=10, gloss=50)

    lens = scene.circle(58, 33, 9)
    _flat_role(scene, palette, lens, "primary")

    stage = scene.line([(24, 61), (46, 49)], 6)
    _solid(scene, stage, with_alpha((83, 139, 255, 255), 255))
    _solid(scene, scene.circle(35, 55, 3), with_alpha((205, 206, 215, 255), 255))


def build_documents(scene: Scene, palette) -> None:
    briefcase = scene.rounded_rect(18, 34, 82, 72, 8)
    _extrude_role(scene, palette, briefcase, "warm", depth=8, shadow_offset=(0, 8), shadow_blur=11, gloss=52)

    flap = scene.line([(19, 47), (50, 47), (81, 47)], 2.3)
    clasp = scene.rounded_rect(46, 49, 54, 57, 2)
    handle = union_masks(scene.circle(50, 29, 6), scene.rounded_rect(44, 27, 56, 34, 3))
    _solid(scene, flap, with_alpha((105, 70, 80, 255), 140))
    _extrude_role(scene, palette, clasp, "accent", depth=4, shadow_offset=(0, 4), shadow_blur=6, gloss=40)
    _extrude_role(scene, palette, handle, "neutral", depth=4, shadow_offset=(0, 4), shadow_blur=6, gloss=36)

    back_tab = scene.rounded_rect(26, 28, 44, 38, 4)
    _flat_role(scene, palette, back_tab, "neutral")


def build_edit(scene: Scene, palette) -> None:
    body = union_masks(
        scene.line([(27, 73), (71, 29)], 16),
        scene.circle(27, 73, 7),
        scene.circle(71, 29, 7),
    )
    _extrude_role(scene, palette, body, "warm", depth=8, shadow_offset=(1, 8), shadow_blur=11, gloss=60)

    ferrule = scene.rounded_rect(67, 25, 75, 33, 2)
    _extrude_role(scene, palette, ferrule, "neutral", depth=4, shadow_offset=(0, 4), shadow_blur=5, gloss=35)

    eraser = scene.rounded_rect(70, 18, 81, 29, 4)
    _extrude_role(scene, palette, eraser, "danger", depth=4, shadow_offset=(0, 4), shadow_blur=5, gloss=40)

    tip = scene.polygon([(20, 80), (29, 71), (38, 80), (29, 89)])
    _flat_role(scene, palette, tip, "neutral")
    lead = scene.polygon([(26, 78), (29, 75), (32, 78), (29, 81)])
    _solid(scene, lead, with_alpha((72, 46, 92, 255), 255))


def build_http(scene: Scene, palette) -> None:
    envelope = scene.rounded_rect(18, 32, 82, 75, 9)
    _extrude_role(scene, palette, envelope, "neutral", depth=6, shadow_offset=(0, 7), shadow_blur=10, gloss=60)

    flap_left = scene.line([(22, 37), (50, 58)], 2.8)
    flap_right = scene.line([(78, 37), (50, 58)], 2.8)
    _solid(scene, flap_left, with_alpha((207, 188, 220, 255), 180))
    _solid(scene, flap_right, with_alpha((207, 188, 220, 255), 180))

    arrow = union_masks(
        scene.rounded_rect(45, 19, 55, 49, 4),
        scene.polygon([(39, 44), (61, 44), (50, 60)]),
    )
    _extrude_role(scene, palette, arrow, "danger", depth=6, shadow_offset=(0, 7), shadow_blur=8, gloss=64)


def build_message(scene: Scene, palette) -> None:
    bubble_body = scene.rounded_rect(18, 24, 81, 69, 11)
    tail = scene.polygon([(28, 69), (40, 69), (24, 83)])
    bubble = union_masks(bubble_body, tail)
    _extrude_role(scene, palette, bubble, "primary", depth=8, shadow_offset=(0, 8), shadow_blur=11, gloss=66)

    line1 = scene.line([(31, 40), (63, 40)], 2.4)
    line2 = scene.line([(31, 49), (55, 49)], 2.4)
    line3 = scene.line([(31, 58), (49, 58)], 2.4)
    for line in (line1, line2, line3):
        _solid(scene, line, with_alpha((255, 255, 255, 255), 180))

    dot = scene.circle(68, 57, 3)
    _flat_role(scene, palette, dot, "accent")


def build_pdf(scene: Scene, palette) -> None:
    page, fold, seam = _folded_page(scene, 24, 22, 76, 79, 11, 11)
    _extrude_role(scene, palette, page, "neutral", depth=7, shadow_offset=(0, 8), shadow_blur=10, gloss=60)
    _flat_role(scene, palette, fold, "danger")
    _solid(scene, seam, with_alpha((255, 255, 255, 255), 150))

    badge = scene.rounded_rect(43, 31, 70, 61, 6)
    _extrude_role(scene, palette, badge, "danger", depth=4, shadow_offset=(0, 4), shadow_blur=6, gloss=52)
    badge_bar = scene.line([(48, 39), (63, 39)], 2.7)
    badge_bar2 = scene.line([(48, 45), (62, 45)], 2.7)
    badge_bar3 = scene.line([(48, 51), (60, 51)], 2.7)
    for bar in (badge_bar, badge_bar2, badge_bar3):
        _solid(scene, bar, with_alpha((255, 255, 255, 255), 180))


def build_remind_list(scene: Scene, palette) -> None:
    board = scene.rounded_rect(24, 27, 76, 80, 10)
    _extrude_role(scene, palette, board, "neutral", depth=7, shadow_offset=(0, 8), shadow_blur=10, gloss=58)

    pin_head = scene.circle(50, 19, 7)
    pin_stem = scene.rounded_rect(48, 18, 52, 31, 2)
    pin = union_masks(pin_head, pin_stem)
    _extrude_role(scene, palette, pin, "warm", depth=5, shadow_offset=(0, 5), shadow_blur=6, gloss=56)

    bullets = (
        scene.circle(35, 42, 2),
        scene.circle(35, 52, 2),
        scene.circle(35, 62, 2),
    )
    for bullet in bullets:
        _flat_role(scene, palette, bullet, "secondary")

    line1 = scene.line([(42, 42), (67, 42)], 2.4)
    line2 = scene.line([(42, 52), (62, 52)], 2.4)
    line3 = scene.line([(42, 62), (58, 62)], 2.4)
    for line in (line1, line2, line3):
        _solid(scene, line, with_alpha((126, 114, 144, 255), 200))

    check = scene.line([(31, 67), (36, 72), (44, 64)], 3.0)
    _solid(scene, check, with_alpha((91, 194, 119, 255), 255))


def build_task(scene: Scene, palette) -> None:
    tile = scene.rounded_rect(22, 24, 78, 80, 14)
    _extrude_role(scene, palette, tile, "secondary", depth=8, shadow_offset=(0, 8), shadow_blur=11, gloss=60)

    check = scene.line([(33, 53), (45, 65), (67, 39)], 6.5)
    _solid(scene, check, with_alpha((245, 247, 255, 255), 255))

    sheen = scene.line([(33, 41), (60, 41)], 2.2)
    _solid(scene, sheen, with_alpha((255, 255, 255, 255), 95))


def build_web_fetch(scene: Scene, palette) -> None:
    globe = scene.circle(47, 45, 27)
    _extrude_role(scene, palette, globe, "primary", depth=9, shadow_offset=(0, 9), shadow_blur=12, gloss=70)

    grid = union_masks(
        scene.line([(20, 45), (74, 45)], 2.4),
        scene.line([(47, 18), (47, 72)], 2.4),
        scene.line([(29, 29), (65, 61)], 2.0),
        scene.line([(29, 61), (65, 29)], 2.0),
        outline_mask(scene.circle(47, 45, 18), scene.px(2)),
    )
    _solid(scene, intersect_mask(grid, globe), with_alpha((255, 255, 255, 255), 145))

    orbit = outline_mask(scene.ellipse(47, 45, 36, 18), scene.px(3))
    _flat_role(scene, palette, orbit, "accent", alpha=230)

    fetch = union_masks(
        scene.rounded_rect(69, 45, 76, 72, 3),
        scene.polygon([(61, 62), (84, 62), (72.5, 80)]),
    )
    _extrude_role(scene, palette, fetch, "secondary", depth=6, shadow_offset=(0, 7), shadow_blur=8, gloss=56)
    dock = scene.rounded_rect(60, 81, 85, 86, 3)
    _flat_role(scene, palette, dock, "neutral", alpha=210)


def build_file_write(scene: Scene, palette) -> None:
    page = scene.rounded_rect(23, 20, 75, 82, 11)
    fold_cut = scene.polygon([(62, 20), (75, 20), (75, 33)])
    page = subtract_mask(page, fold_cut)
    _extrude_role(scene, palette, page, "neutral", depth=8, shadow_offset=(0, 8), shadow_blur=10, gloss=60)

    fold = scene.polygon([(62, 20), (75, 33), (62, 33)])
    _flat_role(scene, palette, fold, "primary")
    _solid(scene, scene.line([(62, 33), (74, 33)], 1.4), with_alpha((255, 255, 255, 255), 145))

    for y, right in ((42, 58), (51, 64), (60, 52)):
        _solid(scene, scene.line([(32, y), (right, y)], 2.2), with_alpha((115, 129, 149, 255), 170))

    pen_body = union_masks(
        scene.line([(38, 76), (72, 42)], 12),
        scene.circle(38, 76, 5.5),
        scene.circle(72, 42, 5.5),
    )
    _extrude_role(scene, palette, pen_body, "secondary", depth=7, shadow_offset=(1, 7), shadow_blur=9, gloss=58)

    nib = scene.polygon([(27, 87), (38, 76), (47, 85), (35, 94)])
    _extrude_role(scene, palette, nib, "warm", depth=5, shadow_offset=(0, 5), shadow_blur=6, gloss=48)
    _solid(scene, scene.circle(36, 84, 2.1), with_alpha((70, 45, 91, 255), 255))

    cap = scene.rounded_rect(69, 35, 80, 46, 3)
    _extrude_role(scene, palette, cap, "danger", depth=4, shadow_offset=(0, 4), shadow_blur=5, gloss=44)


def build_write(scene: Scene, palette) -> None:
    body = union_masks(
        scene.line([(25, 72), (70, 27)], 15),
        scene.circle(25, 72, 6.5),
        scene.circle(70, 27, 6.5),
    )
    _extrude_role(scene, palette, body, "secondary", depth=8, shadow_offset=(1, 8), shadow_blur=11, gloss=58)

    nib = scene.polygon([(17, 81), (29, 69), (39, 79), (27, 91)])
    _extrude_role(scene, palette, nib, "warm", depth=5, shadow_offset=(0, 5), shadow_blur=6, gloss=48)
    nib_cut = scene.polygon([(25, 80), (30, 75), (35, 80), (30, 85)])
    _solid(scene, nib_cut, with_alpha((68, 40, 90, 255), 255))

    ferrule = scene.rounded_rect(65, 22, 75, 33, 2)
    _extrude_role(scene, palette, ferrule, "neutral", depth=4, shadow_offset=(0, 4), shadow_blur=5, gloss=35)

    cap = scene.rounded_rect(68, 17, 81, 29, 4)
    _extrude_role(scene, palette, cap, "danger", depth=4, shadow_offset=(0, 4), shadow_blur=5, gloss=42)

    baseline = scene.rounded_rect(20, 82, 55, 87, 2.5)
    _flat_role(scene, palette, baseline, "primary", alpha=210)


ICON_DEFINITIONS: list[IconSpec] = [
    icon("file_write_3d.png", "sky", build_file_write),
    icon("search_3d.png", "sky", build_search),
    icon("tool_cron_3d.png", "amber", build_cron, depth_bias=1),
    icon("tool_doc_extract_3d.png", "crimson", build_doc_extract),
    icon("tool_doc_read_3d.png", "sky", build_doc_read),
    icon("tool_doc_search_3d.png", "violet", build_doc_search, depth_bias=1),
    icon("tool_documents_3d.png", "coral", build_documents),
    icon("tool_edit_3d.png", "amber", build_edit),
    icon("tool_http_3d.png", "crimson", build_http),
    icon("tool_message_3d.png", "violet", build_message),
    icon("tool_pdf_3d.png", "slate", build_pdf),
    icon("tool_remind_list_3d.png", "violet", build_remind_list),
    icon("tool_task_3d.png", "mint", build_task),
    icon("tool_web_fetch_3d.png", "violet", build_web_fetch, depth_bias=1),
    icon("tool_write_3d.png", "sky", build_write),
]
