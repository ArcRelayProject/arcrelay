"""Approve allowlisted maintainers and request GitHub's protected auto-merge.

Run only from the default branch. Never fetch or execute pull request code.
The merge itself remains subject to required checks and branch protection.
"""
import json
import os
import subprocess
import sys

TRUSTED_AUTHORS = {"zibo-chen": 58510061, "chenzibo": 18285974}
# CI runs every job on every PR. CodeQL default setup analyzes these five languages.
# Require their registration as well as SUCCESS, so a not-yet-started job cannot be skipped.
# The optional CodeQL aggregate is checked when reported, not required to exist.
EXPECTED_TECHNICAL_CHECKS = {
    "check",
    "windows-clipboard",
    "windows-system-folders",
    "windows-share-target",
    "windows-share-target-arm64",
    "Analyze (actions)",
    "Analyze (csharp)",
    "Analyze (javascript-typescript)",
    "Analyze (python)",
    "Analyze (rust)",
}


def eligible(pr, repository):
    author = pr.get("user", {})
    return (
        pr.get("state") == "open"
        and not pr.get("draft", False)
        and pr.get("base", {}).get("ref") == "main"
        and pr.get("base", {}).get("repo", {}).get("full_name") == repository
        and author.get("login") in TRUSTED_AUTHORS
        and author.get("id") == TRUSTED_AUTHORS[author["login"]]
    )


def gh(*args, payload=None):
    command = ["gh", *args]
    if payload is not None:
        command.extend(["--input", "-"])
    result = subprocess.run(
        command, input=json.dumps(payload) if payload is not None else None,
        text=True, check=False, capture_output=True,
    )
    if result.returncode:
        print(result.stderr, file=sys.stderr)
        result.check_returncode()
    return result.stdout


def quality_checks_passed(repository, number, expected_head):
    # Wait for the configured technical jobs and all other reported checks,
    # in addition to the existing protected merge gate.
    snapshot = json.loads(gh("pr", "view", str(number), "--repo", repository,
                             "--json", "headRefOid,statusCheckRollup"))
    if snapshot["headRefOid"] != expected_head:
        return False
    checks = snapshot["statusCheckRollup"]
    checks = [check for check in checks
              if check.get("workflowName") != "Trusted maintainer auto-merge"]
    reported = {check.get("name", check.get("context")) for check in checks}
    if not EXPECTED_TECHNICAL_CHECKS.issubset(reported):
        return False
    return all(
        (check.get("status") == "COMPLETED"
         and check.get("conclusion") == "SUCCESS")
        if "status" in check else check.get("state") == "SUCCESS"
        for check in checks
    )


def configure(repository, number):
    endpoint = f"repos/{repository}/pulls/{number}"
    pr = json.loads(gh("api", endpoint))
    if not eligible(pr, repository):
        print(f"PR #{number}: not an eligible maintainer PR; leaving policy unchanged.")
        return
    sha = pr["head"]["sha"]
    if not quality_checks_passed(repository, number, sha):
        print(f"PR #{number}: technical checks are missing, pending, or unsuccessful; waiting.")
        return
    # Bind approval and auto-merge to the same immutable head. A new push
    # dismisses the approval and triggers this workflow again.
    reviews = json.loads(gh("api", f"{endpoint}/reviews?per_page=100"))
    bot_reviews = [r for r in reviews if r["user"]["login"] == "github-actions[bot]" and r["state"] != "COMMENTED"]
    if not bot_reviews or bot_reviews[-1]["state"] != "APPROVED" or bot_reviews[-1]["commit_id"] != sha:
        gh("api", f"{endpoint}/reviews", "--method", "POST", payload={
            "event": "APPROVE", "commit_id": sha,
            "body": "Automated approval for an allowlisted maintainer. Required CI, CodeQL, and branch protections still apply.",
        })
    gh("pr", "merge", str(number), "--repo", repository, "--auto", "--squash", "--match-head-commit", sha)
    print(f"PR #{number}: protected auto-merge requested for {sha}.")

    # Merges performed with GITHUB_TOKEN do not emit a push event that starts
    # another workflow. Explicitly dispatch main CI once this invocation has
    # completed the merge so the workflow_run-based Test release can continue.
    merged_pr = json.loads(gh("api", endpoint))
    if merged_pr.get("merged"):
        gh(
            "api",
            f"repos/{repository}/actions/workflows/ci.yml/dispatches",
            "--method",
            "POST",
            payload={"ref": "main"},
        )
        print(f"PR #{number}: dispatched main CI for merge {merged_pr['merge_commit_sha']}.")


def main():
    repository = os.environ["GITHUB_REPOSITORY"]
    with open(os.environ["GITHUB_EVENT_PATH"], encoding="utf-8") as source:
        event = json.load(source)
    if "pull_request" in event:
        numbers = [event["pull_request"]["number"]]
    else:
        prs = json.loads(gh("pr", "list", "--repo", repository, "--base", "main", "--state", "open", "--limit", "1000", "--json", "number"))
        numbers = [pr["number"] for pr in prs]
    for number in numbers:
        configure(repository, number)


if __name__ == "__main__":
    main()
