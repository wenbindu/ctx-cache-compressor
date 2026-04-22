-- wrk script for concurrent append load against ctx-cache-compressor
-- usage:
-- SESSION_COUNT=1000 wrk -t4 -c1000 -d30s -s scripts/append_load.lua http://127.0.0.1:8080

math.randomseed(os.time())

counter = 0
session_count = tonumber(os.getenv("SESSION_COUNT") or "1000")
session_roles = {}

function request()
  counter = counter + 1
  local sid_num = ((counter - 1) % session_count) + 1
  local sid = "load-" .. tostring(sid_num)
  local role = session_roles[sid]

  if role == nil then
    role = "user"
  end

  local content = role .. " message #" .. tostring(counter)
  session_roles[sid] = (role == "user") and "assistant" or "user"

  local body = string.format('{"role":"%s","content":"%s"}', role, content)
  local path = "/sessions/" .. sid .. "/messages"

  wrk.method = "POST"
  wrk.path = path
  wrk.body = body
  wrk.headers["Content-Type"] = "application/json"

  return wrk.format(wrk.method, wrk.path, wrk.headers, wrk.body)
end
