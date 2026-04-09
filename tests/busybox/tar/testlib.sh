normalize_tar_listing() {
	python3 -c '
import re
import sys

for raw in sys.stdin:
    line = raw.rstrip("\n")
    match = re.search(r"([^ ]+) link to (.*)$", line)
    if match:
        print(f"{match.group(1)} -> {match.group(2)}")
        continue
    match = re.search(r"([^ ]+) -> (.*)$", line)
    if match:
        print(f"{match.group(1)} -> {match.group(2)}")
        continue
    match = re.search(r"([^ ]+)$", line)
    if match:
        print(match.group(1))
'
}

normalize_ls_listing() {
	python3 -c '
import re
import sys

for raw in sys.stdin:
    line = raw.rstrip("\n")
    match = re.match(r"^([^ @]+)@?.* ([^ ]+) -> ([^ ]+)$", line)
    if match:
        print(f"{match.group(1)} {match.group(2)} -> {match.group(3)}")
        continue
    match = re.match(r"^([^ @]+)@?.* ([^ ]+)$", line)
    if match:
        print(f"{match.group(1)} {match.group(2)}")
'
}
