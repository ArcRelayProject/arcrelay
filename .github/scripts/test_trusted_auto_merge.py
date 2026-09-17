import copy
import unittest
from unittest.mock import patch
import trusted_auto_merge as policy


class TrustedAutoMergeTests(unittest.TestCase):
    def setUp(self):
        readiness = patch.object(policy, "quality_checks_passed", return_value=True)
        self.ready = readiness.start()
        self.addCleanup(readiness.stop)
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


class QualityCheckTests(unittest.TestCase):
    @patch.object(policy, "gh")
    def test_pending_failed_and_missing_checks_wait(self, gh):
        import json
        for checks in [[], [{"name": "check", "status": "IN_PROGRESS"}],
                       [{"name": "check", "status": "COMPLETED", "conclusion": "FAILURE"}],
                       [{"name": "check", "status": "COMPLETED", "conclusion": "NEUTRAL"}],
                       [{"name": "check", "status": "COMPLETED", "conclusion": "SKIPPED"}],
                       [{"name": "check", "status": "COMPLETED", "conclusion": "SUCCESS"},
                        {"name": "Analyze (rust)", "status": "COMPLETED", "conclusion": "NEUTRAL"}],
                       [{"name": "check", "status": "COMPLETED", "conclusion": "SUCCESS"},
                        {"name": "Analyze (rust)", "status": "IN_PROGRESS"}]]:
            gh.return_value = json.dumps({"headRefOid": "a" * 40, "statusCheckRollup": checks})
            self.assertFalse(policy.quality_checks_passed("owner/repo", 1, "a" * 40))

    @patch.object(policy, "gh")
    def test_successful_checks_ignore_only_own_workflow(self, gh):
        import json
        checks = [{"name": name, "status": "COMPLETED", "conclusion": "SUCCESS"}
                  for name in policy.EXPECTED_TECHNICAL_CHECKS]
        checks.append({"workflowName": "Trusted maintainer auto-merge", "status": "IN_PROGRESS"})
        gh.return_value = json.dumps({"headRefOid": "a" * 40, "statusCheckRollup": checks})
        self.assertTrue(policy.quality_checks_passed("owner/repo", 1, "a" * 40))

    @patch.object(policy, "gh")
    def test_every_configured_technical_check_must_be_reported(self, gh):
        import json
        for missing in policy.EXPECTED_TECHNICAL_CHECKS:
            checks = [{"name": name, "status": "COMPLETED", "conclusion": "SUCCESS"}
                      for name in policy.EXPECTED_TECHNICAL_CHECKS if name != missing]
            gh.return_value = json.dumps({"headRefOid": "a" * 40, "statusCheckRollup": checks})
            self.assertFalse(policy.quality_checks_passed("owner/repo", 1, "a" * 40))

    @patch.object(policy, "gh")
    def test_changed_head_never_reuses_successful_checks(self, gh):
        import json
        gh.return_value = json.dumps({"headRefOid": "b" * 40, "statusCheckRollup": [
            {"name": "check", "status": "COMPLETED", "conclusion": "SUCCESS"}]})
        self.assertFalse(policy.quality_checks_passed("owner/repo", 1, "a" * 40))

    @patch.object(policy, "quality_checks_passed", return_value=False)
    @patch.object(policy, "gh")
    def test_waiting_checks_never_approve_or_request_merge(self, gh, ready):
        import json
        gh.return_value = json.dumps({"state": "open", "draft": False,
            "user": {"login": "zibo-chen", "id": 58510061},
            "base": {"ref": "main", "repo": {"full_name": "owner/repo"}},
            "head": {"sha": "a" * 40}})
        policy.configure("owner/repo", 1)
        self.assertEqual(gh.call_count, 1)


class DesktopQualityCheckTests(unittest.TestCase):
    def successful_checks(self):
        return [{"name": name, "status": "COMPLETED", "conclusion": "SUCCESS"}
                for name in policy.EXPECTED_TECHNICAL_CHECKS]

    def test_every_desktop_ci_job_is_explicitly_required(self):
        import re
        from pathlib import Path
        workflow = (Path(__file__).parents[1] / "workflows" / "ci.yml").read_text()
        jobs = set(re.findall(r"^  ([a-z][a-z0-9-]+):$", workflow.split("jobs:\n", 1)[1], re.MULTILINE))
        expected = {name for name in policy.EXPECTED_TECHNICAL_CHECKS if not name.startswith("Analyze (")}
        self.assertEqual(jobs, expected)

    @patch.object(policy, "gh")
    def test_a_failed_or_unfinished_windows_job_blocks_otherwise_successful_checks(self, gh):
        import json
        for status, conclusion in [("QUEUED", ""), ("IN_PROGRESS", ""),
                                   ("COMPLETED", "FAILURE"), ("COMPLETED", "NEUTRAL"),
                                   ("COMPLETED", "SKIPPED"), ("COMPLETED", "CANCELLED"),
                                   ("COMPLETED", "TIMED_OUT"), ("COMPLETED", "ACTION_REQUIRED")]:
            with self.subTest(status=status, conclusion=conclusion):
                checks = self.successful_checks()
                windows = next(check for check in checks if check["name"] == "windows-clipboard")
                windows.update(status=status, conclusion=conclusion)
                gh.return_value = json.dumps({"headRefOid": "a" * 40, "statusCheckRollup": checks})
                self.assertFalse(policy.quality_checks_passed("owner/repo", 1, "a" * 40))

    @patch.object(policy, "gh")
    def test_additional_reported_checks_must_succeed_even_when_not_in_the_required_set(self, gh):
        import json
        for extra in [
            {"name": "CodeQL", "status": "IN_PROGRESS", "conclusion": ""},
            {"name": "CodeQL", "status": "COMPLETED", "conclusion": "NEUTRAL"},
            {"name": "review", "status": "COMPLETED", "conclusion": "FAILURE"},
            {"name": "cla", "status": "COMPLETED", "conclusion": "SKIPPED"},
            {"context": "external-policy", "state": "PENDING"},
        ]:
            with self.subTest(extra=extra):
                gh.return_value = json.dumps({"headRefOid": "a" * 40, "statusCheckRollup": self.successful_checks() + [extra]})
                self.assertFalse(policy.quality_checks_passed("owner/repo", 1, "a" * 40))
        for extra in [[], [{"name": "CodeQL", "status": "COMPLETED", "conclusion": "SUCCESS"}],
                      [{"context": "external-policy", "state": "SUCCESS"}]]:
            gh.return_value = json.dumps({"headRefOid": "a" * 40, "statusCheckRollup": self.successful_checks() + extra})
            self.assertTrue(policy.quality_checks_passed("owner/repo", 1, "a" * 40))

    @patch.object(policy, "gh")
    def test_changed_head_with_all_checks_green_still_waits(self, gh):
        import json
        gh.return_value = json.dumps({"headRefOid": "b" * 40, "statusCheckRollup": self.successful_checks()})
        self.assertFalse(policy.quality_checks_passed("owner/repo", 1, "a" * 40))


if __name__ == "__main__":
    unittest.main()
