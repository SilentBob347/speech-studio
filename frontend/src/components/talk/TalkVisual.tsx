import Aura from "./Aura";
import Orb from "./Orb";
import Wave from "./Wave";
import type { TalkStyle, VisualProps } from "./visual";

/** The talk screen's picture in the chosen look. */
export default function TalkVisual({ look, ...props }: VisualProps & { look: TalkStyle }) {
  if (look === "aura") return <Aura {...props} />;
  if (look === "wave") return <Wave {...props} />;
  return <Orb {...props} />;
}
