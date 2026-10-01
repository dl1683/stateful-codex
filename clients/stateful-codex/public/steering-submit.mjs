// Steering is the user's instruction to a running outcome; losing it silently is worse than
// refusing it. The draft is cleared only after the server returns the saved steering record;
// every other outcome keeps the text and leaves a visible, persistent explanation.
export async function submitSteering({ state, control, drafts, rpc, action }) {
  if (!control.value.trim()) return false;
  if (!state.run) {
    state.steeringError =
      "The run is still being prepared, so this steering was not sent. Your text is kept; submit it again once the run controls appear.";
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
      state.steering = [
        ...state.steering.filter((item) => item.id !== response.steering.id),
        response.steering,
      ];
      saved = true;
    });
  } catch (error) {
    state.steeringError = `Steering was not saved: ${error.message} Your text is kept.`;
    return false;
  }
  if (saved) state.steeringError = null;
  return saved;
}
