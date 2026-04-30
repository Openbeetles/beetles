import Box from "@mui/material/Box";
import Typography from "@mui/material/Typography";

const SKILL_NAME_PREFIXES = ["runtime_skill__", "runtime_skill_", "skill__", "skill_"];
const SKILL_BADGE_HUES = [214, 194, 164, 142, 36, 18, 346, 292, 266, 228] as const;

function stripSkillPrefix(name: string): string {
  const matched = SKILL_NAME_PREFIXES.find((prefix) => name.startsWith(prefix));
  return matched ? name.slice(matched.length) : name;
}

function skillMonogramSeed(name: string): string {
  return stripSkillPrefix(name).trim() || name.trim() || "skill";
}

function skillMonogramGlyph(name: string): string {
  const seed = skillMonogramSeed(name);
  const matched = seed.match(/[A-Za-z0-9]/);
  return matched ? matched[0].toUpperCase() : "S";
}

function skillMonogramTone(name: string) {
  const seed = skillMonogramSeed(name);
  const hash = [...seed].reduce(
    (acc, char, index) => (acc * 33 + char.charCodeAt(0) + index * 17) % 9973,
    17,
  );
  const hue = SKILL_BADGE_HUES[hash % SKILL_BADGE_HUES.length];
  return {
    outerSurface: `color-mix(in srgb, hsl(${hue} 78% 60%) 9%, var(--card))`,
    innerSurface: `color-mix(in srgb, hsl(${hue} 76% 58%) 16%, var(--surface))`,
    text: `color-mix(in srgb, hsl(${hue} 82% 46%) 72%, var(--foreground))`,
    border: `color-mix(in srgb, hsl(${hue} 72% 58%) 24%, var(--border))`,
    shadow: `hsl(${hue} 72% 54%)`,
  };
}

export function SkillMonogramBadge({ name }: { name: string }) {
  const glyph = skillMonogramGlyph(name);
  const tone = skillMonogramTone(name);

  return (
    <Box
      sx={{
        width: 52,
        height: 52,
        borderRadius: "calc(var(--radius-control) + 4px)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        flexShrink: 0,
        backgroundColor: tone.outerSurface,
        backgroundImage:
          "linear-gradient(180deg, color-mix(in srgb, #fff 38%, transparent) 0%, transparent 22%, transparent 100%)",
        border: "1px solid color-mix(in srgb, #fff 52%, var(--border))",
        boxShadow: [
          "inset 0 1px 0 color-mix(in srgb, #fff 78%, transparent)",
          "inset 0 -1px 0 color-mix(in srgb, var(--foreground) 4%, transparent)",
          `0 18px 24px -22px color-mix(in srgb, ${tone.shadow} 44%, transparent)`,
          "0 8px 16px -14px color-mix(in srgb, var(--foreground) 18%, transparent)",
        ].join(", "),
      }}
    >
      <Box
        sx={{
          width: 36,
          height: 36,
          borderRadius: "calc(var(--radius-control) + 1px)",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          backgroundColor: tone.innerSurface,
          backgroundImage: [
            "linear-gradient(180deg, color-mix(in srgb, #fff 70%, transparent) 0%, transparent 58%)",
            `linear-gradient(145deg, color-mix(in srgb, ${tone.shadow} 14%, transparent) 0%, transparent 100%)`,
          ].join(", "),
          border: `1px solid ${tone.border}`,
          boxShadow: [
            "inset 0 1px 0 color-mix(in srgb, #fff 72%, transparent)",
            "inset 0 -1px 0 color-mix(in srgb, var(--foreground) 5%, transparent)",
            `0 8px 12px -10px color-mix(in srgb, ${tone.shadow} 24%, transparent)`,
          ].join(", "),
        }}
      >
        <Typography
          component="span"
          sx={{
            fontFamily: "var(--font-brand)",
            fontSize: "1.12rem",
            fontWeight: 800,
            lineHeight: 1,
            letterSpacing: 0,
            color: tone.text,
            textShadow:
              "0 1px 0 color-mix(in srgb, #fff 44%, transparent), 0 4px 8px color-mix(in srgb, var(--foreground) 10%, transparent)",
            userSelect: "none",
          }}
        >
          {glyph}
        </Typography>
      </Box>
    </Box>
  );
}
