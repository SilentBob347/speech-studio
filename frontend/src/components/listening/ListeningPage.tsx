import { PROJECT_KEY, useOpenProject } from "../../lib/openProject";
import ListeningHome from "./ListeningHome";
import ListenEditor from "./ListenEditor";

/** Listening: the open recording, or the start screen. */
export default function ListeningPage() {
  const [pid, open] = useOpenProject(PROJECT_KEY.listening);
  return pid ? <ListenEditor key={pid} pid={pid} onBack={() => open(null)} onOpen={open} /> : <ListeningHome onOpen={open} />;
}
