import { useOpenProject } from "../../lib/openProject";
import ProjectsHome from "./ProjectsHome";
import ProjectEditor from "./ProjectEditor";

/** Speech: the open project, or the start screen. */
export default function SpeechPage() {
  const [pid, open] = useOpenProject("speech-studio:speech-project");
  return pid ? <ProjectEditor key={pid} pid={pid} onBack={() => open(null)} /> : <ProjectsHome onOpen={open} />;
}
