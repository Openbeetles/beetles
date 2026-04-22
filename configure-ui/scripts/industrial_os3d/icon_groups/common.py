from __future__ import annotations

from dataclasses import dataclass
from functools import lru_cache
from pathlib import Path
from typing import Callable, Iterable, Literal

from PIL import Image, ImageChops, ImageDraw, ImageFilter

Color = tuple[int, int, int, int]
Builder = Callable[["Scene", "Palette"], None]
RoleName = Literal["primary", "secondary", "accent", "warm", "danger", "neutral"]
ChassisMode = Literal["none", "panel"]

CONFIGURE_UI_ROOT = Path(__file__).resolve().parents[3]
PUBLIC_ICON_DIR = CONFIGURE_UI_ROOT / "public" / "icons"
BASE_CANVAS_SIZE = 256
ICON_OUTPUT_SIZE = 512
ICON_SUPERSAMPLE_SCALE = 2
ICON_RENDER_SIZE = ICON_OUTPUT_SIZE * ICON_SUPERSAMPLE_SCALE


@dataclass(frozen=True)
class Palette:
    shell_top: Color
    shell_mid: Color
    shell_bottom: Color
    panel_top: Color
    panel_bottom: Color
    primary_top: Color
    primary_bottom: Color
    secondary_top: Color
    secondary_bottom: Color
    accent_top: Color
    accent_bottom: Color
    warm_top: Color
    warm_bottom: Color
    danger_top: Color
    danger_bottom: Color
    shadow: Color
    glow: Color


@dataclass(frozen=True)
class IconSpec:
    name: str
    palette: str
    builder: Builder
    depth_bias: int = 0
    chassis: ChassisMode = "none"


PALETTES: dict[str, Palette] = {
    "sky": Palette(
        shell_top=(250, 252, 255, 255),
        shell_mid=(186, 197, 211, 255),
        shell_bottom=(78, 93, 109, 255),
        panel_top=(236, 244, 255, 255),
        panel_bottom=(196, 211, 230, 255),
        primary_top=(87, 207, 255, 255),
        primary_bottom=(42, 110, 255, 255),
        secondary_top=(129, 232, 193, 255),
        secondary_bottom=(44, 168, 111, 255),
        accent_top=(255, 210, 95, 255),
        accent_bottom=(255, 143, 62, 255),
        warm_top=(255, 184, 143, 255),
        warm_bottom=(209, 100, 62, 255),
        danger_top=(255, 116, 116, 255),
        danger_bottom=(210, 55, 72, 255),
        shadow=(28, 40, 58, 120),
        glow=(73, 131, 255, 72),
    ),
    "teal": Palette(
        shell_top=(250, 252, 255, 255),
        shell_mid=(184, 198, 203, 255),
        shell_bottom=(79, 95, 101, 255),
        panel_top=(236, 249, 246, 255),
        panel_bottom=(193, 221, 216, 255),
        primary_top=(92, 235, 216, 255),
        primary_bottom=(25, 176, 161, 255),
        secondary_top=(93, 165, 255, 255),
        secondary_bottom=(55, 102, 221, 255),
        accent_top=(198, 255, 142, 255),
        accent_bottom=(92, 199, 86, 255),
        warm_top=(255, 211, 120, 255),
        warm_bottom=(236, 146, 62, 255),
        danger_top=(255, 118, 112, 255),
        danger_bottom=(216, 58, 73, 255),
        shadow=(22, 55, 58, 118),
        glow=(49, 189, 170, 70),
    ),
    "amber": Palette(
        shell_top=(255, 252, 245, 255),
        shell_mid=(212, 198, 178, 255),
        shell_bottom=(110, 96, 79, 255),
        panel_top=(255, 245, 221, 255),
        panel_bottom=(230, 208, 163, 255),
        primary_top=(255, 218, 119, 255),
        primary_bottom=(255, 152, 61, 255),
        secondary_top=(255, 255, 186, 255),
        secondary_bottom=(223, 201, 94, 255),
        accent_top=(121, 223, 255, 255),
        accent_bottom=(59, 133, 221, 255),
        warm_top=(255, 188, 151, 255),
        warm_bottom=(221, 118, 74, 255),
        danger_top=(255, 130, 102, 255),
        danger_bottom=(216, 66, 74, 255),
        shadow=(64, 44, 22, 112),
        glow=(255, 166, 76, 68),
    ),
    "mint": Palette(
        shell_top=(248, 252, 247, 255),
        shell_mid=(190, 206, 190, 255),
        shell_bottom=(86, 100, 86, 255),
        panel_top=(234, 248, 236, 255),
        panel_bottom=(196, 223, 201, 255),
        primary_top=(164, 241, 165, 255),
        primary_bottom=(71, 178, 94, 255),
        secondary_top=(119, 221, 238, 255),
        secondary_bottom=(45, 154, 184, 255),
        accent_top=(255, 218, 120, 255),
        accent_bottom=(247, 156, 56, 255),
        warm_top=(255, 184, 150, 255),
        warm_bottom=(214, 106, 89, 255),
        danger_top=(255, 126, 120, 255),
        danger_bottom=(208, 61, 78, 255),
        shadow=(34, 58, 36, 110),
        glow=(74, 187, 96, 68),
    ),
    "violet": Palette(
        shell_top=(251, 250, 255, 255),
        shell_mid=(195, 190, 212, 255),
        shell_bottom=(88, 84, 111, 255),
        panel_top=(243, 239, 255, 255),
        panel_bottom=(208, 198, 233, 255),
        primary_top=(177, 149, 255, 255),
        primary_bottom=(104, 82, 218, 255),
        secondary_top=(119, 215, 255, 255),
        secondary_bottom=(52, 144, 231, 255),
        accent_top=(255, 205, 122, 255),
        accent_bottom=(250, 145, 68, 255),
        warm_top=(255, 180, 170, 255),
        warm_bottom=(220, 110, 109, 255),
        danger_top=(255, 130, 140, 255),
        danger_bottom=(211, 64, 98, 255),
        shadow=(42, 35, 70, 118),
        glow=(106, 88, 219, 72),
    ),
    "coral": Palette(
        shell_top=(255, 249, 248, 255),
        shell_mid=(215, 189, 186, 255),
        shell_bottom=(112, 84, 84, 255),
        panel_top=(255, 238, 234, 255),
        panel_bottom=(232, 195, 187, 255),
        primary_top=(255, 162, 133, 255),
        primary_bottom=(234, 97, 86, 255),
        secondary_top=(255, 221, 132, 255),
        secondary_bottom=(249, 157, 69, 255),
        accent_top=(128, 230, 244, 255),
        accent_bottom=(54, 153, 198, 255),
        warm_top=(255, 199, 160, 255),
        warm_bottom=(219, 117, 86, 255),
        danger_top=(255, 129, 129, 255),
        danger_bottom=(215, 61, 76, 255),
        shadow=(63, 37, 39, 114),
        glow=(234, 97, 86, 74),
    ),
    "slate": Palette(
        shell_top=(248, 250, 253, 255),
        shell_mid=(186, 195, 205, 255),
        shell_bottom=(72, 83, 97, 255),
        panel_top=(232, 238, 246, 255),
        panel_bottom=(192, 205, 220, 255),
        primary_top=(145, 164, 184, 255),
        primary_bottom=(84, 102, 124, 255),
        secondary_top=(125, 213, 255, 255),
        secondary_bottom=(60, 134, 229, 255),
        accent_top=(168, 232, 180, 255),
        accent_bottom=(77, 182, 115, 255),
        warm_top=(255, 206, 134, 255),
        warm_bottom=(238, 145, 63, 255),
        danger_top=(255, 126, 126, 255),
        danger_bottom=(210, 59, 74, 255),
        shadow=(26, 37, 48, 120),
        glow=(81, 110, 148, 62),
    ),
    "crimson": Palette(
        shell_top=(255, 250, 250, 255),
        shell_mid=(210, 190, 198, 255),
        shell_bottom=(101, 79, 89, 255),
        panel_top=(255, 239, 242, 255),
        panel_bottom=(232, 201, 208, 255),
        primary_top=(255, 139, 166, 255),
        primary_bottom=(218, 76, 113, 255),
        secondary_top=(255, 210, 126, 255),
        secondary_bottom=(249, 150, 69, 255),
        accent_top=(142, 219, 255, 255),
        accent_bottom=(57, 138, 223, 255),
        warm_top=(255, 186, 153, 255),
        warm_bottom=(219, 112, 94, 255),
        danger_top=(255, 118, 118, 255),
        danger_bottom=(204, 56, 74, 255),
        shadow=(52, 30, 41, 120),
        glow=(218, 76, 113, 70),
    ),
}

AUTO_FIT_THRESHOLD_ALPHA = 96
AUTO_FIT_BOX = (28, 22, 228, 222)


def icon(
    name: str,
    palette: str,
    builder: Builder,
    depth_bias: int = 0,
    chassis: ChassisMode = "none",
) -> IconSpec:
    return IconSpec(
        name=name,
        palette=palette,
        builder=builder,
        depth_bias=depth_bias,
        chassis=chassis,
    )


def rgba(hex_value: str, alpha: int = 255) -> Color:
    hex_value = hex_value.lstrip("#")
    return tuple(int(hex_value[i : i + 2], 16) for i in (0, 2, 4)) + (alpha,)


def blend(a: Color, b: Color, t: float) -> Color:
    t = max(0.0, min(1.0, t))
    return tuple(int(round(x + (y - x) * t)) for x, y in zip(a, b))  # type: ignore[return-value]


def with_alpha(color: Color, alpha: int) -> Color:
    return (color[0], color[1], color[2], alpha)


def lighten(color: Color, amount: float) -> Color:
    return blend(color, (255, 255, 255, color[3]), amount)


def darken(color: Color, amount: float) -> Color:
    return blend(color, (0, 0, 0, color[3]), amount)


def tone(palette: Palette, role: RoleName) -> tuple[Color, Color, Color]:
    if role == "primary":
        return palette.primary_top, palette.primary_bottom, darken(palette.primary_bottom, 0.26)
    if role == "secondary":
        return palette.secondary_top, palette.secondary_bottom, darken(palette.secondary_bottom, 0.24)
    if role == "accent":
        return palette.accent_top, palette.accent_bottom, darken(palette.accent_bottom, 0.24)
    if role == "warm":
        return palette.warm_top, palette.warm_bottom, darken(palette.warm_bottom, 0.24)
    if role == "danger":
        return palette.danger_top, palette.danger_bottom, darken(palette.danger_bottom, 0.24)
    if role in PALETTES:
        return palette.primary_top, palette.primary_bottom, darken(palette.primary_bottom, 0.26)
    neutral_top = lighten(palette.shell_mid, 0.26)
    neutral_bottom = darken(palette.shell_mid, 0.08)
    neutral_side = darken(palette.shell_bottom, 0.08)
    return neutral_top, neutral_bottom, neutral_side


@lru_cache(maxsize=256)
def vertical_gradient(size: int, top: Color, bottom: Color) -> Image.Image:
    image = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)
    for y in range(size):
        t = y / max(1, size - 1)
        draw.line((0, y, size, y), fill=blend(top, bottom, t))
    return image


@lru_cache(maxsize=128)
def vertical_mask(size: int, top_alpha: int, bottom_alpha: int) -> Image.Image:
    image = Image.new("L", (size, size), 0)
    draw = ImageDraw.Draw(image)
    for y in range(size):
        t = y / max(1, size - 1)
        alpha = int(round(top_alpha + (bottom_alpha - top_alpha) * t))
        draw.line((0, y, size, y), fill=alpha)
    return image


def blank_mask(size: int) -> Image.Image:
    return Image.new("L", (size, size), 0)


def union_masks(*masks: Image.Image) -> Image.Image:
    if not masks:
        raise ValueError("union_masks requires at least one mask")
    result = masks[0].copy()
    for mask in masks[1:]:
        result = ImageChops.lighter(result, mask)
    return result


def subtract_mask(base: Image.Image, *cuts: Image.Image) -> Image.Image:
    result = base.copy()
    for cut in cuts:
        result = ImageChops.subtract(result, cut)
    return result


def intersect_mask(a: Image.Image, b: Image.Image) -> Image.Image:
    return ImageChops.multiply(a, b)


def shift_mask(mask: Image.Image, dx: int = 0, dy: int = 0) -> Image.Image:
    shifted = blank_mask(mask.size[0])
    shifted.paste(mask, (dx, dy))
    return shifted


def outline_mask(mask: Image.Image, width: int) -> Image.Image:
    if width <= 0:
        return blank_mask(mask.size[0])
    expanded = mask.filter(ImageFilter.MaxFilter(width * 2 + 1))
    return ImageChops.subtract(expanded, mask)


def scale_box(box: tuple[int, int, int, int], scale: float) -> tuple[int, int, int, int]:
    return tuple(int(round(value * scale)) for value in box)  # type: ignore[return-value]


class Scene:
    def __init__(
        self,
        palette: Palette,
        size: int = ICON_RENDER_SIZE,
        depth_bias: int = 0,
        chassis: ChassisMode = "none",
    ):
        self.palette = palette
        self.size = size
        self.scale = size / BASE_CANVAS_SIZE
        self.depth_bias = depth_bias
        self.chassis = chassis
        self.image = Image.new("RGBA", (size, size), (0, 0, 0, 0))
        if chassis == "panel":
            self.content = scale_box((56, 42, 200, 170), self.scale)
            self.body = scale_box((34, 24, 222, 190), self.scale)
            self.panel = scale_box((48, 38, 208, 174), self.scale)
            self.pedestal = scale_box((42, 180, 214, 220), self.scale)
            self._render_chassis()
        else:
            self.content = scale_box((18, 10, 238, 232), self.scale)
            self.body = None
            self.panel = None
            self.pedestal = None

    def px(self, value: float, minimum: int = 1) -> int:
        return max(minimum, int(round(value * self.scale)))

    def offset_px(self, value: float) -> int:
        return int(round(value * self.scale))

    def pt(self, x: float, y: float) -> tuple[int, int]:
        width = self.content[2] - self.content[0]
        height = self.content[3] - self.content[1]
        return (
            int(round(self.content[0] + (x / 100.0) * width)),
            int(round(self.content[1] + (y / 100.0) * height)),
        )

    def box(self, x1: float, y1: float, x2: float, y2: float) -> tuple[int, int, int, int]:
        px1, py1 = self.pt(x1, y1)
        px2, py2 = self.pt(x2, y2)
        return (px1, py1, px2, py2)

    def scalar(self, value: float) -> int:
        width = self.content[2] - self.content[0]
        return max(1, int(round(width * value / 100.0)))

    def rounded_rect(self, x1: float, y1: float, x2: float, y2: float, radius: float) -> Image.Image:
        mask = blank_mask(self.size)
        ImageDraw.Draw(mask).rounded_rectangle(self.box(x1, y1, x2, y2), radius=self.scalar(radius), fill=255)
        return mask

    def circle(self, cx: float, cy: float, radius: float) -> Image.Image:
        mask = blank_mask(self.size)
        px, py = self.pt(cx, cy)
        r = self.scalar(radius)
        ImageDraw.Draw(mask).ellipse((px - r, py - r, px + r, py + r), fill=255)
        return mask

    def ellipse(self, cx: float, cy: float, rx: float, ry: float) -> Image.Image:
        mask = blank_mask(self.size)
        px, py = self.pt(cx, cy)
        rx_px = self.scalar(rx)
        ry_px = self.scalar(ry)
        ImageDraw.Draw(mask).ellipse((px - rx_px, py - ry_px, px + rx_px, py + ry_px), fill=255)
        return mask

    def polygon(self, points: Iterable[tuple[float, float]]) -> Image.Image:
        mask = blank_mask(self.size)
        ImageDraw.Draw(mask).polygon([self.pt(x, y) for x, y in points], fill=255)
        return mask

    def line(self, points: Iterable[tuple[float, float]], width: float) -> Image.Image:
        mask = blank_mask(self.size)
        ImageDraw.Draw(mask).line([self.pt(x, y) for x, y in points], fill=255, width=self.scalar(width), joint="curve")
        return mask

    def arc_band(
        self,
        cx: float,
        cy: float,
        outer_radius: float,
        inner_radius: float,
        start: float,
        end: float,
    ) -> Image.Image:
        outer = self.circle(cx, cy, outer_radius)
        inner = self.circle(cx, cy, inner_radius)
        ring = subtract_mask(outer, inner)
        wedge = blank_mask(self.size)
        px, py = self.pt(cx, cy)
        r = self.scalar(outer_radius)
        ImageDraw.Draw(wedge).pieslice((px - r, py - r, px + r, py + r), start=start, end=end, fill=255)
        return intersect_mask(ring, wedge)

    def render_extruded(
        self,
        mask: Image.Image,
        top: Color,
        bottom: Color,
        side: Color,
        depth: int = 10,
        shadow: Color | None = None,
        shadow_offset: tuple[int, int] = (0, 10),
        shadow_blur: int = 10,
        gloss: int = 70,
    ) -> None:
        depth = max(2, self.px(depth + self.depth_bias))
        shadow_offset = (
            self.offset_px(shadow_offset[0]),
            self.offset_px(shadow_offset[1]),
        )
        shadow_blur = self.px(shadow_blur)
        shadow_color = shadow or with_alpha(self.palette.shadow, 100)
        shadow_mask = shift_mask(mask, dx=shadow_offset[0], dy=shadow_offset[1]).filter(ImageFilter.GaussianBlur(shadow_blur))
        self._paste_fill(shadow_mask, with_alpha(shadow_color, shadow_color[3]))

        side_mask = shift_mask(mask, dy=depth)
        self._paste_fill(side_mask, blend(lighten(side, 0.08), side, 0.65))
        self._paste_fill(mask, vertical_gradient(self.size, top, bottom))

        spec_mask = intersect_mask(mask, vertical_mask(self.size, gloss, 0)).filter(ImageFilter.GaussianBlur(self.px(3)))
        self._paste_fill(spec_mask, with_alpha((255, 255, 255, 255), int(gloss * 0.62)))

    def render_flat(self, mask: Image.Image, top: Color, bottom: Color | None = None, alpha: int | None = None) -> None:
        fill = vertical_gradient(self.size, top, bottom or top)
        if alpha is not None:
            mask = ImageChops.multiply(mask, Image.new("L", (self.size, self.size), alpha))
        self.image.alpha_composite(Image.new("RGBA", (self.size, self.size), (0, 0, 0, 0)))
        self.image.alpha_composite(Image.composite(fill, Image.new("RGBA", (self.size, self.size), (0, 0, 0, 0)), mask))

    def render_outline(self, mask: Image.Image, color: Color, width: int = 3) -> None:
        self._paste_fill(outline_mask(mask, self.px(width)), color)

    def render_glow(self, mask: Image.Image, color: Color | None = None, blur: int = 18, alpha: int = 88) -> None:
        glow = mask.filter(ImageFilter.GaussianBlur(self.px(blur)))
        self._paste_fill(glow, with_alpha(color or self.palette.glow, alpha))

    def _paste_fill(self, mask: Image.Image, fill: Image.Image | Color) -> None:
        if isinstance(fill, tuple):
            source = Image.new("RGBA", (self.size, self.size), fill)
        else:
            source = fill
        self.image.alpha_composite(Image.composite(source, Image.new("RGBA", (self.size, self.size), (0, 0, 0, 0)), mask))

    def _render_chassis(self) -> None:
        body_mask = blank_mask(self.size)
        ImageDraw.Draw(body_mask).rounded_rectangle(self.body, radius=self.px(34), fill=255)
        body_side = darken(self.palette.shell_bottom, 0.18)
        self.render_extruded(body_mask, self.palette.shell_top, self.palette.shell_mid, body_side, depth=16, shadow_blur=18)

        rim_mask = outline_mask(body_mask, self.px(2))
        self._paste_fill(rim_mask, with_alpha((255, 255, 255, 255), 120))

        panel_mask = blank_mask(self.size)
        ImageDraw.Draw(panel_mask).rounded_rectangle(self.panel, radius=self.px(28), fill=255)
        self.render_flat(panel_mask, self.palette.panel_top, self.palette.panel_bottom)
        inner_rim = outline_mask(panel_mask, self.px(2))
        self._paste_fill(inner_rim, with_alpha((255, 255, 255, 255), 92))

        pedestal_mask = blank_mask(self.size)
        ImageDraw.Draw(pedestal_mask).rounded_rectangle(self.pedestal, radius=self.px(22), fill=255)
        self.render_extruded(
            pedestal_mask,
            lighten(self.palette.shell_top, 0.05),
            lighten(self.palette.shell_mid, 0.08),
            darken(self.palette.shell_bottom, 0.14),
            depth=10,
            shadow_blur=14,
            shadow_offset=(0, 12),
            gloss=58,
        )
        self.render_glow(pedestal_mask, alpha=42, blur=12)


def render_icon(spec: IconSpec) -> Image.Image:
    palette = PALETTES[spec.palette]
    scene = Scene(palette, size=ICON_RENDER_SIZE, depth_bias=spec.depth_bias, chassis=spec.chassis)
    spec.builder(scene, palette)
    fitted = auto_fit_icon(scene.image, spec.chassis)
    if fitted.size != (ICON_OUTPUT_SIZE, ICON_OUTPUT_SIZE):
        fitted = fitted.resize((ICON_OUTPUT_SIZE, ICON_OUTPUT_SIZE), Image.Resampling.LANCZOS)
    return fitted


def auto_fit_icon(image: Image.Image, chassis: ChassisMode) -> Image.Image:
    if chassis == "panel":
        return image.resize((ICON_OUTPUT_SIZE, ICON_OUTPUT_SIZE), Image.Resampling.LANCZOS)

    alpha = image.getchannel("A")
    threshold = alpha.point(lambda value: 255 if value >= AUTO_FIT_THRESHOLD_ALPHA else 0)
    bbox = threshold.getbbox() or alpha.getbbox()
    if bbox is None:
        return image

    x1, y1, x2, y2 = bbox
    content_width = x2 - x1
    content_height = y2 - y1
    if content_width <= 0 or content_height <= 0:
        return image

    target_x1, target_y1, target_x2, target_y2 = scale_box(AUTO_FIT_BOX, image.width / BASE_CANVAS_SIZE)
    target_width = target_x2 - target_x1
    target_height = target_y2 - target_y1
    scale = min(target_width / content_width, target_height / content_height)
    scaled_canvas_width = max(1, int(round(image.width * scale)))
    scaled_canvas_height = max(1, int(round(image.height * scale)))
    resized = image.resize(
        (scaled_canvas_width, scaled_canvas_height),
        Image.Resampling.LANCZOS,
    )

    fitted = Image.new("RGBA", image.size, (0, 0, 0, 0))
    target_center_x = (target_x1 + target_x2) / 2
    target_center_y = (target_y1 + target_y2) / 2
    scaled_bbox_center_x = ((x1 + x2) * scale) / 2
    scaled_bbox_center_y = ((y1 + y2) * scale) / 2
    offset_x = int(round(target_center_x - scaled_bbox_center_x))
    offset_y = int(round(target_center_y - scaled_bbox_center_y))
    fitted.alpha_composite(resized, (offset_x, offset_y))
    return fitted
