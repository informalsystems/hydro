import json
import os

print("Generating full schema...")

schema = {
    "instantiate": {},
    "execute": {},
    "query": {},
    "migrate": {},
    "sudo": {},
    "responses": {},
}

required = [
    "instantiate_msg.json",
    "execute_msg.json",
    "query_msg.json",
]

responses = []


files = [f for f in os.listdir(".") if os.path.isfile(f)]
for f in files:
    if f.endswith("_response.json"):
        responses.append(f)

print("Using response files:", responses)

with open("instantiate_msg.json", "r") as imj:
    schema["instantiate"] = json.load(imj)

with open("execute_msg.json", "r") as exj:
    schema["execute"] = json.load(exj)

with open("query_msg.json", "r") as qj:
    schema["query"] = json.load(qj)

for file in responses:
    with open(file, "r") as f:
        schema["responses"][file.split("_response.json")[0]] = json.load(f)

with open("dao_voting_adapter_full_schema.json", "w") as f:
    json.dump(schema, f, indent=2)

print("DAO Voting Adapter full schema generated.")
