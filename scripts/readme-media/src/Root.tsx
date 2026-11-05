import { Composition, Folder, Still } from "remotion";
import { Hero } from "./Hero";
import { ThemeTour } from "./ThemeTour";

export const RemotionRoot: React.FC = () => {
  return (
    <>
      <Folder name="Banners">
        <Still id="hero-ru" component={Hero} width={1280} height={640} defaultProps={{ lang: "ru" as const }} />
        <Still id="hero-en" component={Hero} width={1280} height={640} defaultProps={{ lang: "en" as const }} />
      </Folder>
      <Folder name="Tours">
        <Composition
          id="themes-ru"
          component={ThemeTour}
          width={800}
          height={520}
          fps={12}
          durationInFrames={120}
          defaultProps={{ lang: "ru" as const }}
        />
        <Composition
          id="themes-en"
          component={ThemeTour}
          width={800}
          height={520}
          fps={12}
          durationInFrames={120}
          defaultProps={{ lang: "en" as const }}
        />
      </Folder>
    </>
  );
};
