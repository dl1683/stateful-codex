// Steering is the user's instruction to a running outcome; losing it silently is worse than
// refusing it. The draft is cleared only after the server returns the saved steering record;
// every other outcome keeps the text and leaves a visible, persistent explanation.
export async function submitSteering({ state, control, drafts, rpc, action }) {
  const text = control.value.trim();
  if (!text) return false;
  if (!state.run) {
    state.steeringError =
      "The run is still being prepared, so this steering was not sent. Your text is kept; submit it again once the run controls appear.";
    return false;
  }
  if (drafts.isSubmitting(control)) {
    state.steeringError =
      "Earlier steering is still being sent, so this text was not sent. It is kept; submit it again once the earlier steering is saved.";
    return false;
  }
  const runId = state.run.id;
  let saved = false;
  try {
    await drafts.submit(control, async (input) => {
      const response = await action("Applying steering", () =>
        rpc("steering/submit", {
          runId,
          input,
          affectedObligationIds: state.obligations.at(-1)
            ? [state.obligations.at(-1).id]
            : [],
          idempotencyKey: crypto.randomUUID(),
        }),
      );
      if (!response?.steering?.id) {
        throw new Error("The server did not confirm that it saved the steering.");
      }
      const others = (items) => items.filter((item) => item.id !== response.steering.id);
      state.steering = [...others(state.steering), response.steering];
      state.confirmedSteering = [...others(state.confirmedSteering ?? []), response.steering];
      saved = true;
    });
  } catch (error) {
    // If the form was replaced meanwhile (for example the run closed), the text survives here.
    state.steeringError = control.isConnected
      ? `Steering was not saved: ${error.message} Your text is kept.`
      : `Steering was not saved: ${error.message} Your text was: ${text}`;
    return false;
  }
  if (saved) state.steeringError = null;
  return saved;
}
