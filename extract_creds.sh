#!/usr/bin/env bash

set -euo pipefail

_trim() {
  local s=$1
  shopt -s extglob
  s="${s##+([[:space:]])}"
  s="${s%%+([[:space:]])}"
  printf '%s' "$s"
}

_unquote_curl_arg() {
  local s
  s=$(_trim "$1")
  if [[ ${s: -1} == '\' ]]; then
    s=${s:0:${#s}-1}
    s=$(_trim "$s")
  fi
  if [[ ${#s} -ge 2 && ${s:0:1} == "'" && ${s: -1} == "'" ]]; then
    s=${s:1:${#s}-2}
  elif [[ ${#s} -ge 2 && ${s:0:1} == '"' && ${s: -1} == '"' ]]; then
    s=${s:1:${#s}-2}
  fi
  printf '%s' "$s"
}

_shell_quote() {
  printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")"
}

_extract_header() {
  local name=$1 curl=$2 line header
  shopt -s nocasematch
  while IFS= read -r line; do
    case "$line" in
      *"-H "*)
        header=$(_unquote_curl_arg "${line#*"-H "}")
        ;;
      *"--header "*)
        header=$(_unquote_curl_arg "${line#*"--header "}")
        ;;
      *)
        continue
        ;;
    esac
    if [[ $header =~ ^[[:space:]]*$name:[[:space:]]*(.*)$ ]]; then
      printf '%s' "${BASH_REMATCH[1]}"
      return 0
    fi
  done <<< "$curl"
  return 1
}

_extract_cookies() {
  local curl=$1 line cookies
  while IFS= read -r line; do
    case "$line" in
      *"-b "*)
        cookies=$(_unquote_curl_arg "${line#*"-b "}")
        printf '%s' "$cookies"
        return 0
        ;;
      *"--cookie "*)
        cookies=$(_unquote_curl_arg "${line#*"--cookie "}")
        printf '%s' "$cookies"
        return 0
        ;;
    esac
  done <<< "$curl"

  _extract_header "cookie" "$curl"
}

_b64url_decode() {
  local s=$1 rem
  s=${s//-/+}
  s=${s//_/\/}
  rem=$((${#s} % 4))
  case $rem in
    0) ;;
    2) s="${s}==" ;;
    3) s="${s}=" ;;
    *) return 1 ;;
  esac

  if printf '' | base64 --decode >/dev/null 2>&1; then
    printf '%s' "$s" | base64 --decode
  else
    printf '%s' "$s" | base64 -D
  fi
}

_account_id_from_jwt() {
  local token=$1 payload
  payload=${token#*.}
  payload=${payload%%.*}
  _b64url_decode "$payload" | sed -n 's/.*"chatgpt_account_id":"\([^"]*\)".*/\1/p'
}

curl_to_creds_env() {
  local curl token account_id cookies
  if (($#)); then
    curl=$*
  else
    curl=$(cat)
  fi

  token=$({ _extract_header "authorization" "$curl" || true; } | sed -n 's/^[Bb]earer[[:space:]]\{1,\}//p')
  account_id=$(_extract_header "chatgpt-account-id" "$curl" || true)
  cookies=$(_extract_cookies "$curl" || true)

  if [[ -z $token ]]; then
    printf 'missing authorization: Bearer header\n' >&2
    return 1
  fi
  if [[ -z $account_id ]]; then
    # ponytail: decode JWT without verifying the signature; enough for extracting this local config value.
    account_id=$(_account_id_from_jwt "$token")
  fi
  if [[ -z $account_id ]]; then
    printf 'missing chatgpt-account-id header and no chatgpt_account_id claim in token\n' >&2
    return 1
  fi
  if [[ -z $cookies ]]; then
    printf 'missing cookies: expected -b, --cookie, or cookie header\n' >&2
    return 1
  fi

  printf 'TOKEN=%s\n' "$(_shell_quote "$token")"
  printf 'ACCOUNT_ID=%s\n' "$(_shell_quote "$account_id")"
  printf 'COOKIES=%s\n' "$(_shell_quote "$cookies")"
}

_self_test() {
  local payload token out
  payload=$(printf '%s' '{"https://api.openai.com/auth":{"chatgpt_account_id":"acc-from-jwt"}}' |
    base64 | tr -d '=\n' | tr '+/' '-_')
  token="h.${payload}.s"
  out=$(curl_to_creds_env "curl x -H 'authorization: Bearer ${token}' -b 'a=1; b=two'")

  [[ $out == *"TOKEN='${token}'"* ]]
  [[ $out == *"ACCOUNT_ID='acc-from-jwt'"* ]]
  [[ $out == *"COOKIES='a=1; b=two'"* ]]
  printf 'ok\n'
}

if [[ ${BASH_SOURCE[0]} == "$0" ]]; then
  if [[ ${1:-} == "--self-test" ]]; then
    _self_test
  else
    curl_to_creds_env "$@"
  fi
fi
