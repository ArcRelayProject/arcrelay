import copy
import unittest
from unittest.mock import patch
import trusted_auto_merge as policy


class TrustedAutoMergeTests(unittest.TestCase):
    def setUp(self):
        self.pr = {"state": "open", "draft": False, "user": {"login": "zibo-chen", "id": 58510061}, "base": {"ref": "main", "repo": {"full_name": "ArcRelayProject/arcrelay"}}, "head": {"sha": "a" * 40}}
        self.repo = "ArcRelayProject/arcrelay"

    def test_both_accounts_are_allowed(self):
        for login, identifier in policy.TRUSTED_AUTHORS.items():
            self.pr["user"] = {"login": login, "id": identifier}
            self.assertTrue(policy.eligible(self.pr, self.repo))

    def test_untrusted_authors_drafts_wrong_base_and_renamed_accounts_are_rejected(self):
        for field, value in [("user", {"login": "outsider", "id": 1}), ("user", {"login": "zibo-chen", "id": 1}), ("draft", True), ("state", "closed"), ("base", {"ref": "release", "repo": {"full_name": self.repo}}), ("base", {"ref": "main", "repo": {"full_name": "other/repository"}})]:
            candidate = copy.deepcopy(self.pr)
            candidate[field] = value
            self.assertFalse(policy.eligible(candidate, self.repo))

    @patch.object(policy, "gh")
    def test_untrusted_pr_is_never_approved_or_merged(self, gh):
        import json
        self.pr["user"] = {"login": "outsider", "id": 1}
        gh.return_value = json.dumps(self.pr)
        policy.configure(self.repo, 12)
        self.assertEqual(gh.call_count, 1)

    @patch.object(policy, "gh")
    def test_approval_and_auto_merge_are_bound_to_same_head_without_admin_bypass(self, gh):
        import json
        merged = {"merged": True, "merge_commit_sha": "c" * 40}
        gh.side_effect = [json.dumps(self.pr), "[]", "{}", "", json.dumps(merged), ""]
        policy.configure(self.repo, 12)
        calls = gh.call_args_list
        self.assertEqual(calls[2].kwargs["payload"]["commit_id"], self.pr["head"]["sha"])
        self.assertEqual(calls[2].kwargs["payload"]["event"], "APPROVE")
        self.assertIn("--auto", calls[3].args)
        self.assertNotIn("--admin", calls[3].args)
        self.assertEqual(calls[3].args[-2:], ("--match-head-commit", self.pr["head"]["sha"]))
        self.assertEqual(calls[5].args[1], "repos/ArcRelayProject/arcrelay/actions/workflows/ci.yml/dispatches")
        self.assertEqual(calls[5].kwargs["payload"], {"ref": "main"})

    @patch.object(policy, "gh")
    def test_main_ci_is_not_dispatched_while_auto_merge_is_pending(self, gh):
        import json
        gh.side_effect = [json.dumps(self.pr), "[]", "{}", "", json.dumps({"merged": False})]
        policy.configure(self.repo, 12)
        self.assertEqual(gh.call_count, 5)
        self.assertFalse(any("dispatches" in " ".join(call.args) for call in gh.call_args_list))

    @patch.object(policy, "gh")
    def test_current_approval_is_reused_but_stale_approval_is_replaced(self, gh):
        import json
        for commit, expected_calls in [(self.pr["head"]["sha"], 5), ("b" * 40, 6)]:
            gh.reset_mock()
            review = {"user": {"login": "github-actions[bot]"}, "state": "APPROVED", "commit_id": commit}
            merged = {"merged": True, "merge_commit_sha": "c" * 40}
            approval = [] if commit == self.pr["head"]["sha"] else [""]
            gh.side_effect = [json.dumps(self.pr), json.dumps([review]), *approval, "", json.dumps(merged), ""]
            policy.configure(self.repo, 12)
            self.assertEqual(gh.call_count, expected_calls)


if __name__ == "__main__":
    unittest.main()
