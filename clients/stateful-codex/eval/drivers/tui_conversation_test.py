import unittest

from tui_conversation import approval_prompt, composer_pending, is_busy, stable_idle


class ConversationDriverTests(unittest.TestCase):
    def test_busy_markers_are_explicit(self):
        self.assertTrue(is_busy("Working (12s)"))
        self.assertFalse(is_busy("Ask Codex >"))

    def test_approval_detection_does_not_accept_arbitrary_text(self):
        self.assertTrue(approval_prompt("Would you like to run the following command?"))
        self.assertFalse(approval_prompt("The report would you like to see this?"))

    def test_idle_requires_visible_screen_and_quiet_period(self):
        self.assertFalse(stable_idle("", 30, 16))
        self.assertFalse(stable_idle("Working (1s)", 30, 16))
        self.assertFalse(stable_idle("Press enter to confirm or esc to cancel", 30, 16))
        self.assertTrue(stable_idle("Ask Codex >", 16, 16))

    def test_mcp_enter_to_submit_is_an_approval(self):
        self.assertTrue(approval_prompt("Allow the local MCP server to run tool foo? enter to submit"))

    def test_composer_recovery_detects_typed_and_pasted_content(self):
        self.assertTrue(composer_pending("Ask Codex › next message", "next message"))
        self.assertTrue(composer_pending("� next message", "next message"))
        self.assertTrue(composer_pending("Ask Codex › [Pasted Content 42 chars]", "next message"))
        self.assertFalse(composer_pending("Ask Codex", "next message"))


if __name__ == "__main__":
    unittest.main()
