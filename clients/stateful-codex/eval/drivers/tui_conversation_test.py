import unittest

from tui_conversation import approval_prompt, is_busy, stable_idle


class ConversationDriverTests(unittest.TestCase):
    def test_busy_markers_are_explicit(self):
        self.assertTrue(is_busy("Working (12s)"))
        self.assertFalse(is_busy("Ask Codex >"))

    def test_approval_detection_does_not_accept_arbitrary_text(self):
        self.assertTrue(approval_prompt("Would you like me to run this command?"))
        self.assertFalse(approval_prompt("The report would you like to see this?"))

    def test_idle_requires_visible_screen_and_quiet_period(self):
        self.assertFalse(stable_idle("", 30, 16))
        self.assertFalse(stable_idle("Working (1s)", 30, 16))
        self.assertTrue(stable_idle("Ask Codex >", 16, 16))


if __name__ == "__main__":
    unittest.main()
