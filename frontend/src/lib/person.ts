/** A control that turns the microphone on answers only the person's own click or key: the agent's ui_click is refused
 * on it, and an event the page did not get from the person (isTrusted false) changes nothing. */
export const PERSON_ONLY = { "data-person-only": "" } as const;

export const personOnly = (on: boolean) => (on ? PERSON_ONLY : {});

export const byPerson = (event: { isTrusted: boolean }) => event.isTrusted;
