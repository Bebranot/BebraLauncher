import { loadFont } from "@remotion/google-fonts/JetBrainsMono";
import {
  AbsoluteFill,
  Easing,
  Img,
  Sequence,
  interpolate,
  staticFile,
  useCurrentFrame,
} from "remotion";

const { fontFamily } = loadFont("normal", { weights: ["500"], subsets: ["latin"] });

const THEMES = [
  ["dark", "Dark"],
  ["light", "Light"],
  ["nord", "Nord"],
  ["dracula", "Dracula"],
  ["amber", "Amber"],
  ["forest", "Forest"],
  ["copper_night", "Copper Night"],
  ["classic", "Classic"],
] as const;

const HOLD = 15;
const WIPE = 4;

/** One theme: wipes in over the previous one from the left, then holds. */
const Slide: React.FC<{
  readonly id: string;
  readonly label: string;
  readonly lang: string;
  readonly first: boolean;
}> = ({ id, label, lang, first }) => {
  const frame = useCurrentFrame();
  // The first slide starts fully shown: frame 0 is what static previews of a GIF display.
  const reveal = first
    ? 100
    : interpolate(frame, [0, WIPE], [0, 100], {
        extrapolateRight: "clamp",
        easing: Easing.out(Easing.cubic),
      });
  return (
    <AbsoluteFill style={{ clipPath: `inset(0 ${100 - reveal}% 0 0)` }}>
      <Img src={staticFile(`shots/theme-${id}-${lang}.png`)} style={{ width: "100%" }} />
      <div
        style={{
          position: "absolute",
          left: 240,
          top: 18,
          padding: "5px 11px",
          borderRadius: 6,
          background: "#0D0E12",
          color: "#F2F3F7",
          fontFamily,
          fontWeight: 500,
          fontSize: 15,
        }}
      >
        {label}
      </div>
    </AbsoluteFill>
  );
};

export const ThemeTour: React.FC<{ readonly lang: "ru" | "en" }> = ({ lang }) => (
  <AbsoluteFill style={{ background: "#0D0E12" }}>
    {THEMES.map(([id, label], i) => (
      <Sequence key={id} from={i * HOLD} layout="none">
        <Slide id={id} label={label} lang={lang} first={i === 0} />
      </Sequence>
    ))}
  </AbsoluteFill>
);
