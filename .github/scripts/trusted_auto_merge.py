"""Approve allowlisted maintainers and request GitHub's protected auto-merge.

Run only from the default branch. Never fetch or execute pull request code.
The merge itself remains subject to required checks and branch protection.
"""
import json
import os
import subprocess
import sys

TRUSTED_AUTHORS = {"zibo-chen": 58510061, "chenzibo": 18285974}


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


def configure(repository, number):
    endpoint = f"repos/{repository}/pulls/{number}"
    pr = json.loads(gh("api", endpoint))
    if not eligible(pr, repository):
        print(f"PR #{number}: not an eligible maintainer PR; leaving policy unchanged.")
        return
    sha = pr["head"]["sha"]
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
