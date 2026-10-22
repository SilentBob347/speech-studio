use super::project::{EngineRule, SpeechProject, ValidationRules};

pub(crate) fn validate(project: &SpeechProject) -> Result<(), String> {
    let engines: Vec<_> = crate::engines::all()
        .into_iter()
        .map(|e| EngineRule {
            id: e.id,
            cloud: crate::engines::is_cloud(e.id),
        })
        .collect();
    project.validate(&ValidationRules {
        engines: &engines,
        genders: &crate::voices::GENDERS,
        ages: &crate::voices::AGES,
    })
}
