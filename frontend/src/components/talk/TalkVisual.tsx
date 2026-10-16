import Aura from "./Aura";
import Bands from "./Bands";
import Wave from "./Wave";
import type { TalkStyle, VisualProps } from "./visual";

/** The talk screen's picture in the chosen look. */
export default function TalkVisual({ look, ...props }: VisualProps & { look: TalkStyle }) {
  if (look === "wave") return <Wave {...props} />;
  if (look === "bars" || look === "radial" || look === "matrix") return <Bands look={look} {...props} />;
  return <Aura {...props} />;
}
