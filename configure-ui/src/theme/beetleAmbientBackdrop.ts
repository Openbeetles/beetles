/**
 * 全局底「甲壳虫」意象：大椭圆叠层模拟俯视身体 + 双翅，极淡、随主色/强调色变化；
 * 静态呈现，避免与壳层抢「系统稳态」感。
 * Ambient beetle hint via soft ellipses (body + wings); theme-colored, static.
 */
export const BEETLE_AMBIENT_BACKDROP_SX = {
  position: "fixed" as const,
  inset: 0,
  zIndex: 0,
  pointerEvents: "none" as const,
  opacity: 0.86,
  transform: "none",
  backgroundImage: [
    // 右下：身体（纵向椭圆）
    "radial-gradient(ellipse 22vmin 40vmin at 92% 96%, color-mix(in srgb, var(--primary) 6%, transparent) 0%, transparent 68%)",
    // 右下：左翅（大、偏上）
    "radial-gradient(ellipse 48vmin 30vmin at 76% 88%, color-mix(in srgb, var(--accent) 4.5%, transparent) 0%, transparent 62%)",
    // 右下：右翅
    "radial-gradient(ellipse 44vmin 28vmin at 99% 82%, color-mix(in srgb, var(--primary) 4%, transparent) 0%, transparent 60%)",
    // 左上：较弱镜像，避免画面只重一角
    "radial-gradient(ellipse 18vmin 34vmin at 6% 10%, color-mix(in srgb, var(--primary) 3.2%, transparent) 0%, transparent 65%)",
    "radial-gradient(ellipse 36vmin 22vmin at 22% 4%, color-mix(in srgb, var(--accent) 2.6%, transparent) 0%, transparent 58%)",
    "radial-gradient(ellipse 32vmin 20vmin at 4% 24%, color-mix(in srgb, var(--primary) 2.4%, transparent) 0%, transparent 58%)",
  ].join(", "),
  backgroundSize: "100% 100%, 100% 100%, 100% 100%, 100% 100%, 100% 100%, 100% 100%",
  backgroundRepeat: "no-repeat",
} as const;
