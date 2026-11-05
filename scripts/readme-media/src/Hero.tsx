import { loadFont as loadDisplay } from "@remotion/google-fonts/Unbounded";
import { loadFont as loadMono } from "@remotion/google-fonts/JetBrainsMono";
import { loadFont as loadBody } from "@remotion/google-fonts/NotoSans";
import { AbsoluteFill, Img, staticFile } from "remotion";

const display = loadDisplay("normal", { weights: ["600"], subsets: ["latin"] });
const mono = loadMono("normal", { weights: ["500"], subsets: ["latin", "cyrillic"] });
const body = loadBody("normal", { weights: ["400"], subsets: ["latin", "cyrillic"] });

const HULL = "#0D0E12";
const LINE = "#7C9BFF";
const ACCENT = "#6C8DFF";
const HAZARD = "#E8C547";

const COPY = {
  ru: {
    tagline: "Лёгкий лаунчер для Space Station 14",
    specs: ["WINDOWS 10/11", "ОКОЛО 35 МБ", "RU · EN · UK · DE"],
  },
  en: {
    tagline: "A light launcher for Space Station 14",
    specs: ["WINDOWS 10/11", "ABOUT 35 MB", "RU · EN · UK · DE"],
  },
};

/** One compartment of the deck plan, in tile units. */
type Room = readonly [x: number, y: number, w: number, h: number];

const TILE = 32;
const ROOMS: Room[] = [
  [1, 1, 9, 6],
  [10, 1, 6, 4],
  [16, 1, 11, 7],
  [27, 1, 7, 5],
  [34, 1, 8, 8],
  [1, 7, 5, 6],
  [6, 9, 12, 6],
  [18, 8, 5, 4],
  [23, 8, 11, 6],
  [34, 9, 8, 9],
  [1, 13, 5, 7],
  [6, 15, 7, 5],
  [13, 15, 10, 5],
  [23, 14, 11, 6],
];

/** A station deck drawn as a blueprint: invented, generated from the list above. */
const DeckPlan: React.FC = () => (
  <svg
    width={1344}
    height={672}
    viewBox="0 0 1344 672"
    style={{ position: "absolute", left: -8, top: -8 }}
    fill="none"
  >
    <defs>
      <pattern id="tile" width={TILE} height={TILE} patternUnits="userSpaceOnUse">
        <path d={`M${TILE} 0H0V${TILE}`} stroke={LINE} strokeOpacity={0.055} />
      </pattern>
    </defs>
    <rect width={1344} height={672} fill="url(#tile)" />
    {ROOMS.map(([x, y, w, h]) => {
      const door = x + Math.floor(w / 2);
      // The top wall has a gap: a door.
      const d = `M${door * TILE} ${y * TILE}H${x * TILE}V${(y + h) * TILE}H${(x + w) * TILE}V${y * TILE}H${(door + 1) * TILE}`;
      return (
        <path
          key={`${x}-${y}`}
          d={d}
          stroke={LINE}
          strokeOpacity={0.42}
          strokeWidth={1.5}
        />
      );
    })}
  </svg>
);

/** The diagonal safety band found on every station bulkhead. */
const HazardBand: React.FC<{ readonly style: React.CSSProperties }> = ({ style }) => (
  <div
    style={{
      position: "absolute",
      backgroundImage: `repeating-linear-gradient(-45deg, ${HAZARD} 0 12px, ${HULL} 12px 24px)`,
      ...style,
    }}
  />
);

export const Hero: React.FC<{ readonly lang: "ru" | "en" }> = ({ lang }) => {
  const copy = COPY[lang];
  return (
    <AbsoluteFill style={{ background: HULL, overflow: "hidden" }}>
      <DeckPlan />
      {/* The left side stays quiet so the type reads over the plan. */}
      <AbsoluteFill
        style={{
          background: `linear-gradient(90deg, ${HULL} 26%, ${HULL}00 44%)`,
        }}
      />

      <Img
        src={staticFile(`shots/servers-${lang}.png`)}
        style={{
          position: "absolute",
          left: 482,
          top: 66,
          width: 764,
          borderRadius: 12,
          border: `1px solid ${LINE}55`,
          boxShadow: "0 30px 90px #00000099",
        }}
      />

      <div style={{ position: "absolute", left: 64, top: 64, width: 470 }}>
        <div
          style={{
            display: "grid",
            placeItems: "center",
            width: 64,
            height: 64,
            borderRadius: 16,
            background: ACCENT,
            color: "#14151A",
            fontFamily: display.fontFamily,
            fontWeight: 600,
            fontSize: 36,
          }}
        >
          B
        </div>
        <h1
          style={{
            margin: "52px 0 0",
            color: "#F2F3F7",
            fontFamily: display.fontFamily,
            fontWeight: 600,
            fontSize: 58,
            lineHeight: 1.05,
            letterSpacing: "-0.02em",
          }}
        >
          Bebra
          <br />
          Launcher
        </h1>
        <p
          style={{
            margin: "26px 0 0",
            color: "#B7BAC8",
            fontFamily: body.fontFamily,
            fontSize: 25,
            lineHeight: 1.3,
            maxWidth: 400,
          }}
        >
          {copy.tagline}
        </p>
      </div>

      <div
        style={{
          position: "absolute",
          left: 64,
          bottom: 62,
          display: "flex",
          flexDirection: "column",
          gap: 6,
          color: "#8F93A4",
          fontFamily: mono.fontFamily,
          fontWeight: 500,
          fontSize: 16,
          letterSpacing: "0.06em",
        }}
      >
        {copy.specs.map((spec) => (
          <span key={spec}>{spec}</span>
        ))}
      </div>

      <HazardBand style={{ left: 0, right: 0, bottom: 0, height: 14 }} />
    </AbsoluteFill>
  );
};
