run_case() {
  printf '%-42s => ' "$1|$2"
  ( GITHUB_EVENT_NAME=$1 GITHUB_REF=$2 bash -e /tmp/guard2.sh ) && echo "OK(reached pre)" || echo "BLOCKED"
}
run_case workflow_dispatch refs/heads/main
run_case push refs/heads/main
run_case push refs/heads/feature
run_case pull_request refs/heads/main
