#!/usr/bin/env python3
"""Replay Cabo Club's public state trace and check its discard invariant."""

import argparse
import json
import sys


def check(log):
    if log["version"] != 1:
        raise ValueError("unsupported game log version")

    previous = None
    for entry in log["entries"]:
        sequence = entry["sequence"]
        before, after = entry["before"], entry["after"]
        if previous and (
            sequence != previous["sequence"] + 1 or before != previous["after"]
        ):
            raise ValueError(f"event {sequence}: trace is discontinuous")

        delta = 1 if entry["result"] == "accepted" else 0
        if after["revision"] != before["revision"] + delta:
            raise ValueError(f"event {sequence}: wrong revision delta")
        if delta == 0 and before != after:
            raise ValueError(f"event {sequence}: unsuccessful command mutated the table")

        for state in (before, after):
            count = state["discardCount"]
            if count < 0 or (count == 0) != (state["discardTop"] is None):
                raise ValueError(f"event {sequence}: discard count and rank disagree")
            if state["dealId"] is not None and count == 0:
                raise ValueError(f"event {sequence}: dealt round has an empty discard pile")
            if state["deckCount"] < 0:
                raise ValueError(f"event {sequence}: negative deck count")

        if delta and entry["action"] == "drawDiscard":
            expected_discard = max(1, before["discardCount"] - 1)
            expected_deck = before["deckCount"] - int(before["discardCount"] == 1)
            if after["discardCount"] != expected_discard or after["deckCount"] != expected_deck:
                raise ValueError(f"event {sequence}: incorrect discard pickup or refill")
        previous = entry


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("log", type=argparse.FileType(), help="game log JSON, or - for stdin")
    args = parser.parse_args()
    try:
        log = json.load(args.log)
        check(log)
    except (KeyError, TypeError, ValueError) as error:
        print(f"Game log failed: {error}", file=sys.stderr)
        return 1
    qualifier = " (truncated trace)" if log["truncated"] else ""
    print(f"Verified {len(log['entries'])} public game events{qualifier}.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
