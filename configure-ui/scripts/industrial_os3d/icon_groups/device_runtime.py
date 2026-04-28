from __future__ import annotations

from PIL import ImageDraw

from industrial_os3d.icon_groups.common import (
    IconSpec,
    Palette,
    Scene,
    blank_mask,
    darken,
    icon,
    lighten,
    tone,
    union_masks,
    with_alpha,
)

ICON_GROUP = "device_runtime"

OWNED_ICONS = (
    "alarm_3d.png",
    "calendar_3d.png",
    "camera_3d.png",
    "contacts_3d.png",
    "folder_3d.png",
    "garden_3d.png",
    "keyboard_3d.png",
    "mail_3d.png",
    "microphone_3d.png",
    "shell_3d.png",
    "speaker_3d.png",
    "time_3d.png",
    "tool_device_ctrl_3d.png",
    "tool_i2c_dev_3d.png",
    "tool_sensor_3d.png",
    "firmware_flash_3d.png",
)


def rounded_mask(scene: Scene, x1: float, y1: float, x2: float, y2: float, radius: float):
    mask = blank_mask(scene.size)
    ImageDraw.Draw(mask).rounded_rectangle(scene.box(x1, y1, x2, y2), radius=scene.scalar(radius), fill=255)
    return mask


def circle_mask(scene: Scene, cx: float, cy: float, radius: float):
    return scene.circle(cx, cy, radius)


def extrude(scene: Scene, mask, palette: Palette, role: str, depth: int = 10, shadow_blur: int = 10) -> None:
    top, bottom, side = tone(palette, role)  # type: ignore[arg-type]
    scene.render_extruded(mask, top, bottom, side, depth=depth, shadow_blur=shadow_blur)


def flat(scene: Scene, mask, palette: Palette, role: str, alpha: int | None = None) -> None:
    top, bottom, _ = tone(palette, role)  # type: ignore[arg-type]
    scene.render_flat(mask, top, bottom, alpha=alpha)


def build_alarm(scene: Scene, palette: Palette) -> None:
    body = rounded_mask(scene, 23, 24, 77, 78, 22)
    extrude(scene, body, palette, "neutral", depth=11, shadow_blur=12)

    left_bell = scene.ellipse(30, 18, 10, 7)
    right_bell = scene.ellipse(70, 18, 10, 7)
    extrude(scene, left_bell, palette, "danger", depth=7, shadow_blur=8)
    extrude(scene, right_bell, palette, "danger", depth=7, shadow_blur=8)

    bell_bridge = rounded_mask(scene, 42, 18, 58, 24, 4)
    flat(scene, bell_bridge, palette, "warm")

    face = circle_mask(scene, 50, 53, 18)
    flat(scene, face, palette, "primary")

    hands = scene.line(((50, 53), (50, 41), (61, 57)), 2.5)
    flat(scene, hands, palette, "neutral", alpha=255)

    ticks = scene.line(((50, 38), (50, 35)), 1.5)
    flat(scene, ticks, palette, "neutral", alpha=180)

    feet_left = rounded_mask(scene, 31, 75, 40, 80, 2)
    feet_right = rounded_mask(scene, 60, 75, 69, 80, 2)
    flat(scene, feet_left, palette, "neutral")
    flat(scene, feet_right, palette, "neutral")

    clapper = circle_mask(scene, 50, 68, 4)
    flat(scene, clapper, palette, "danger")


def build_calendar(scene: Scene, palette: Palette) -> None:
    body = rounded_mask(scene, 22, 26, 78, 78, 12)
    extrude(scene, body, palette, "neutral", depth=11, shadow_blur=12)

    header = rounded_mask(scene, 22, 26, 78, 41, 12)
    flat(scene, header, palette, "primary")

    ring_left = rounded_mask(scene, 33, 20, 39, 32, 3)
    ring_right = rounded_mask(scene, 61, 20, 67, 32, 3)
    flat(scene, ring_left, palette, "accent")
    flat(scene, ring_right, palette, "accent")

    grid_cells = []
    for row in range(2):
        for col in range(3):
            x1 = 28 + col * 15
            y1 = 48 + row * 12
            grid_cells.append(rounded_mask(scene, x1, y1, x1 + 9, y1 + 7, 2))
    for cell in grid_cells:
        flat(scene, cell, palette, "neutral")

    date_badge = rounded_mask(scene, 63, 49, 72, 60, 3)
    flat(scene, date_badge, palette, "warm")


def build_camera(scene: Scene, palette: Palette) -> None:
    body = rounded_mask(scene, 20, 34, 80, 76, 14)
    extrude(scene, body, palette, "neutral", depth=10, shadow_blur=12)

    grip = rounded_mask(scene, 20, 40, 28, 70, 5)
    extrude(scene, grip, palette, "secondary", depth=8, shadow_blur=8)

    flash = rounded_mask(scene, 28, 24, 48, 37, 6)
    flat(scene, flash, palette, "accent")

    lens_outer = circle_mask(scene, 52, 55, 17)
    extrude(scene, lens_outer, palette, "primary", depth=8, shadow_blur=8)

    lens_inner = circle_mask(scene, 52, 55, 9)
    flat(scene, lens_inner, palette, "neutral")

    lens_glint = circle_mask(scene, 47, 50, 4)
    flat(scene, lens_glint, palette, "accent", alpha=160)

    shutter = circle_mask(scene, 67, 30, 3)
    flat(scene, shutter, palette, "danger")


def build_contacts(scene: Scene, palette: Palette) -> None:
    book = rounded_mask(scene, 22, 26, 78, 78, 12)
    extrude(scene, book, palette, "neutral", depth=11, shadow_blur=12)

    spine = rounded_mask(scene, 22, 28, 34, 76, 6)
    extrude(scene, spine, palette, "secondary", depth=8, shadow_blur=8)

    tab_one = rounded_mask(scene, 69, 34, 76, 41, 2)
    tab_two = rounded_mask(scene, 69, 45, 76, 52, 2)
    tab_three = rounded_mask(scene, 69, 56, 76, 63, 2)
    flat(scene, tab_one, palette, "accent")
    flat(scene, tab_two, palette, "warm")
    flat(scene, tab_three, palette, "danger")

    card = rounded_mask(scene, 40, 34, 70, 74, 8)
    flat(scene, card, palette, "primary")

    head_one = circle_mask(scene, 48, 48, 6)
    head_two = circle_mask(scene, 62, 48, 6)
    shoulders_one = rounded_mask(scene, 42, 55, 54, 68, 5)
    shoulders_two = rounded_mask(scene, 56, 55, 68, 68, 5)
    bust_one = head_one.copy()
    bust_one.paste(shoulders_one, (0, 0), shoulders_one)
    bust_two = head_two.copy()
    bust_two.paste(shoulders_two, (0, 0), shoulders_two)
    flat(scene, bust_one, palette, "neutral")
    flat(scene, bust_two, palette, "neutral")


def build_folder(scene: Scene, palette: Palette) -> None:
    back = rounded_mask(scene, 20, 40, 80, 78, 12)
    extrude(scene, back, palette, "warm", depth=10, shadow_blur=12)

    tab = rounded_mask(scene, 24, 30, 52, 44, 7)
    flat(scene, tab, palette, "accent")

    sheet = rounded_mask(scene, 58, 32, 74, 58, 5)
    flat(scene, sheet, palette, "neutral")

    label = rounded_mask(scene, 34, 55, 66, 63, 3)
    flat(scene, label, palette, "secondary")


def build_garden(scene: Scene, palette: Palette) -> None:
    pot = rounded_mask(scene, 28, 58, 72, 82, 8)
    extrude(scene, pot, palette, "warm", depth=9, shadow_blur=10)

    soil = scene.ellipse(50, 58, 18, 7)
    flat(scene, soil, palette, "neutral", alpha=150)

    stem = scene.line(((50, 58), (50, 40)), 4)
    flat(scene, stem, palette, "secondary")

    leaf_left = scene.ellipse(39, 43, 12, 7)
    leaf_right = scene.ellipse(61, 40, 13, 7)
    leaf_top = scene.ellipse(50, 33, 10, 6)
    leaf_low = scene.ellipse(50, 47, 9, 5)
    flat(scene, leaf_left, palette, "secondary")
    flat(scene, leaf_right, palette, "accent")
    flat(scene, leaf_top, palette, "primary")
    flat(scene, leaf_low, palette, "secondary", alpha=200)

    bud = circle_mask(scene, 50, 32, 3)
    flat(scene, bud, palette, "accent")


def build_keyboard(scene: Scene, palette: Palette) -> None:
    body = rounded_mask(scene, 13, 35, 87, 78, 13)
    extrude(scene, body, palette, "neutral", depth=10, shadow_blur=12)

    key_specs = [
        (21, 42, 27, 48), (31, 42, 37, 48), (41, 42, 47, 48), (51, 42, 57, 48), (61, 42, 67, 48), (71, 42, 77, 48),
        (18, 52, 24, 58), (28, 52, 34, 58), (38, 52, 44, 58), (48, 52, 54, 58), (58, 52, 64, 58), (68, 52, 74, 58), (78, 52, 83, 58),
        (21, 62, 28, 68), (32, 62, 39, 68), (43, 62, 50, 68), (54, 62, 61, 68), (65, 62, 72, 68), (76, 62, 83, 68),
    ]
    keys = union_masks(*(rounded_mask(scene, *spec, 2) for spec in key_specs))
    flat(scene, keys, palette, "neutral", alpha=220)

    space = rounded_mask(scene, 31, 70, 69, 75, 2.5)
    flat(scene, space, palette, "secondary", alpha=220)

    accent_key = rounded_mask(scene, 72, 62, 83, 68, 2)
    flat(scene, accent_key, palette, "accent", alpha=235)

    glow = scene.line(((21, 39), (77, 39)), 1.4)
    scene.render_flat(glow, with_alpha(lighten(palette.panel_top, 0.18), 120), with_alpha(palette.panel_top, 70))


def build_shell(scene: Scene, palette: Palette) -> None:
    body = rounded_mask(scene, 16, 24, 84, 80, 12)
    extrude(scene, body, palette, "neutral", depth=10, shadow_blur=12)

    titlebar = rounded_mask(scene, 16, 24, 84, 42, 12)
    flat(scene, titlebar, palette, "secondary", alpha=235)

    for index, role in enumerate(("danger", "warm", "accent")):
        flat(scene, circle_mask(scene, 28 + index * 9, 33, 2.5), palette, role)

    screen = rounded_mask(scene, 22, 41, 78, 74, 7)
    scene.render_flat(
        screen,
        darken(palette.shell_bottom, 0.06),
        darken(palette.shell_bottom, 0.28),
    )

    prompt = scene.line(((31, 51), (38, 57), (31, 63)), 3.2)
    flat(scene, prompt, palette, "accent")

    command = rounded_mask(scene, 43, 55, 64, 60, 2)
    flat(scene, command, palette, "primary", alpha=235)

    cursor = rounded_mask(scene, 68, 55, 72, 61, 1.5)
    flat(scene, cursor, palette, "warm")

    reflection = scene.line(((28, 45), (58, 45)), 1.5)
    scene.render_flat(reflection, with_alpha(lighten(palette.panel_top, 0.12), 112), with_alpha(palette.panel_top, 70))


def build_mail(scene: Scene, palette: Palette) -> None:
    envelope = rounded_mask(scene, 20, 34, 80, 78, 10)
    extrude(scene, envelope, palette, "neutral", depth=10, shadow_blur=11)

    flap = scene.polygon(((20, 34), (50, 56), (80, 34), (80, 42), (50, 61), (20, 42)))
    flat(scene, flap, palette, "primary")

    seam_left = scene.line(((20, 34), (50, 56)), 2)
    seam_right = scene.line(((80, 34), (50, 56)), 2)
    seam_bottom = scene.line(((20, 42), (50, 61), (80, 42)), 2)
    flat(scene, seam_left, palette, "neutral", alpha=200)
    flat(scene, seam_right, palette, "neutral", alpha=200)
    flat(scene, seam_bottom, palette, "neutral", alpha=220)

    seal = circle_mask(scene, 50, 56, 4)
    flat(scene, seal, palette, "warm")


def build_microphone(scene: Scene, palette: Palette) -> None:
    head = rounded_mask(scene, 35, 20, 65, 60, 13)
    extrude(scene, head, palette, "danger", depth=8, shadow_blur=8)

    grille_one = scene.line(((41, 32), (59, 32)), 1.7)
    grille_two = scene.line(((41, 39), (59, 39)), 1.7)
    grille_three = scene.line(((41, 46), (59, 46)), 1.7)
    flat(scene, grille_one, palette, "neutral", alpha=180)
    flat(scene, grille_two, palette, "neutral", alpha=180)
    flat(scene, grille_three, palette, "neutral", alpha=180)

    yoke_left = scene.line(((35, 50), (28, 63), (35, 70)), 3.4)
    yoke_right = scene.line(((65, 50), (72, 63), (65, 70)), 3.4)
    flat(scene, union_masks(yoke_left, yoke_right), palette, "secondary")

    stem = rounded_mask(scene, 47, 59, 53, 76, 3)
    extrude(scene, stem, palette, "neutral", depth=8, shadow_blur=8)

    base = rounded_mask(scene, 29, 72, 71, 84, 6)
    extrude(scene, base, palette, "neutral", depth=8, shadow_blur=8)

    stand_pin = scene.line(((50, 60), (50, 73)), 2.2)
    flat(scene, stand_pin, palette, "secondary")


def build_speaker(scene: Scene, palette: Palette) -> None:
    body = rounded_mask(scene, 24, 30, 76, 78, 12)
    extrude(scene, body, palette, "secondary", depth=10, shadow_blur=11)

    cone_outer = circle_mask(scene, 50, 52, 15)
    extrude(scene, cone_outer, palette, "primary", depth=8, shadow_blur=8)

    cone_inner = circle_mask(scene, 50, 52, 8)
    flat(scene, cone_inner, palette, "neutral")

    tweeter = circle_mask(scene, 50, 34, 4)
    flat(scene, tweeter, palette, "accent")

    wave_inner = scene.arc_band(68, 52, 17, 12, 300, 60)
    wave_outer = scene.arc_band(68, 52, 26, 20, 300, 60)
    flat(scene, wave_inner, palette, "secondary", alpha=220)
    flat(scene, wave_outer, palette, "secondary", alpha=170)


def build_time(scene: Scene, palette: Palette) -> None:
    body = circle_mask(scene, 50, 52, 23)
    extrude(scene, body, palette, "primary", depth=10, shadow_blur=11)

    crown = rounded_mask(scene, 45, 22, 55, 33, 4)
    flat(scene, crown, palette, "neutral")

    side_button = rounded_mask(scene, 62, 29, 68, 35, 2)
    flat(scene, side_button, palette, "accent")

    face = circle_mask(scene, 50, 52, 16)
    flat(scene, face, palette, "neutral")

    hands = scene.line(((50, 52), (50, 42), (60, 50)), 2)
    flat(scene, hands, palette, "primary", alpha=255)

    center = circle_mask(scene, 50, 52, 3)
    flat(scene, center, palette, "danger")


def build_tool_device_ctrl(scene: Scene, palette: Palette) -> None:
    base = rounded_mask(scene, 18, 48, 82, 84, 12)
    extrude(scene, base, palette, "neutral", depth=10, shadow_blur=11)

    mount = scene.ellipse(50, 43, 16, 12)
    extrude(scene, mount, palette, "secondary", depth=8, shadow_blur=8)

    shaft = rounded_mask(scene, 47, 22, 53, 45, 2)
    extrude(scene, shaft, palette, "neutral", depth=8, shadow_blur=7)

    knob = circle_mask(scene, 50, 18, 11)
    extrude(scene, knob, palette, "danger", depth=8, shadow_blur=8)

    grip_left = rounded_mask(scene, 22, 56, 31, 72, 4)
    grip_right = rounded_mask(scene, 69, 56, 78, 72, 4)
    flat(scene, grip_left, palette, "accent", alpha=220)
    flat(scene, grip_right, palette, "warm", alpha=220)

    left_button = circle_mask(scene, 32, 61, 4)
    right_button = circle_mask(scene, 68, 61, 4)
    center_button = circle_mask(scene, 50, 67, 3)
    flat(scene, left_button, palette, "accent")
    flat(scene, right_button, palette, "warm")
    flat(scene, center_button, palette, "primary")


def build_tool_i2c_dev(scene: Scene, palette: Palette) -> None:
    board = rounded_mask(scene, 22, 42, 78, 80, 10)
    extrude(scene, board, palette, "neutral", depth=10, shadow_blur=11)

    bus_bar = rounded_mask(scene, 30, 48, 70, 56, 3)
    flat(scene, bus_bar, palette, "secondary")

    chip = rounded_mask(scene, 39, 52, 61, 68, 5)
    extrude(scene, chip, palette, "primary", depth=7, shadow_blur=7)

    pin_left = circle_mask(scene, 34, 45, 4)
    pin_right = circle_mask(scene, 66, 45, 4)
    flat(scene, pin_left, palette, "warm")
    flat(scene, pin_right, palette, "warm")

    contact_left = rounded_mask(scene, 28, 58, 36, 68, 3)
    contact_right = rounded_mask(scene, 64, 58, 72, 68, 3)
    flat(scene, contact_left, palette, "accent")
    flat(scene, contact_right, palette, "accent")

    trace = scene.line(((34, 45), (50, 45), (66, 45)), 2.2)
    flat(scene, trace, palette, "neutral", alpha=210)


def build_tool_sensor(scene: Scene, palette: Palette) -> None:
    pod = rounded_mask(scene, 28, 46, 72, 80, 12)
    extrude(scene, pod, palette, "secondary", depth=10, shadow_blur=11)

    dome = circle_mask(scene, 50, 37, 13)
    extrude(scene, dome, palette, "accent", depth=8, shadow_blur=8)

    lens = circle_mask(scene, 50, 37, 5)
    flat(scene, lens, palette, "neutral")

    ring_inner = scene.arc_band(64, 40, 12, 9, 310, 55)
    ring_outer = scene.arc_band(64, 40, 20, 16, 310, 55)
    flat(scene, ring_inner, palette, "accent", alpha=220)
    flat(scene, ring_outer, palette, "accent", alpha=160)

    stand = rounded_mask(scene, 48, 58, 52, 72, 2)
    flat(scene, stand, palette, "neutral")


def build_firmware_flash(scene: Scene, palette: Palette) -> None:
    chip = rounded_mask(scene, 26, 27, 74, 75, 9)
    extrude(scene, chip, palette, "slate", depth=11, shadow_blur=13)

    pins = []
    for x in (36, 48, 60):
        pins.append(rounded_mask(scene, x - 3, 18, x + 3, 29, 2))
        pins.append(rounded_mask(scene, x - 3, 73, x + 3, 84, 2))
    for y in (38, 50, 62):
        pins.append(rounded_mask(scene, 16, y - 3, 28, y + 3, 2))
        pins.append(rounded_mask(scene, 72, y - 3, 84, y + 3, 2))
    flat(scene, union_masks(*pins), palette, "warm")

    core = rounded_mask(scene, 35, 36, 65, 66, 7)
    extrude(scene, core, palette, "primary", depth=7, shadow_blur=8)

    flash_bolt = scene.polygon(
        (
            (53, 31),
            (39, 54),
            (49, 54),
            (43, 71),
            (62, 47),
            (52, 47),
        )
    )
    scene.render_glow(flash_bolt, with_alpha(tone(palette, "accent")[0], 160), blur=13, alpha=80)
    extrude(scene, flash_bolt, palette, "accent", depth=6, shadow_blur=7)

    cable = scene.line(((27, 86), (39, 76), (48, 76)), 5.4)
    plug = rounded_mask(scene, 18, 82, 30, 92, 3)
    flat(scene, union_masks(cable, plug), palette, "secondary")

    write_dot = circle_mask(scene, 61, 39, 3.2)
    flat(scene, write_dot, palette, "danger")


ICON_DEFINITIONS: list[IconSpec] = [
    icon("alarm_3d.png", "coral", build_alarm, depth_bias=1),
    icon("calendar_3d.png", "sky", build_calendar),
    icon("camera_3d.png", "sky", build_camera),
    icon("contacts_3d.png", "violet", build_contacts),
    icon("folder_3d.png", "amber", build_folder),
    icon("garden_3d.png", "mint", build_garden),
    icon("keyboard_3d.png", "slate", build_keyboard),
    icon("mail_3d.png", "sky", build_mail),
    icon("microphone_3d.png", "crimson", build_microphone, depth_bias=1),
    icon("shell_3d.png", "slate", build_shell),
    icon("speaker_3d.png", "teal", build_speaker),
    icon("time_3d.png", "sky", build_time, depth_bias=1),
    icon("tool_device_ctrl_3d.png", "violet", build_tool_device_ctrl, depth_bias=1),
    icon("tool_i2c_dev_3d.png", "amber", build_tool_i2c_dev),
    icon("tool_sensor_3d.png", "mint", build_tool_sensor),
    icon("firmware_flash_3d.png", "sky", build_firmware_flash, depth_bias=1),
]
