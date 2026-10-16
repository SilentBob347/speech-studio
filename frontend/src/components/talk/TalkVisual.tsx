import Aura from "./Aura";
import Wave from "./Wave";
import type { TalkStyle, VisualProps } from "./visual";

/** The talk screen's picture in the chosen look. */
export default function TalkVisual({ look, ...props }: VisualProps & { look: TalkStyle }) {
  if (look === "wave") return <Wave {...props} />;
  return <Aura {...props} />;
}
