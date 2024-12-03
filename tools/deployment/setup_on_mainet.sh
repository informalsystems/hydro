#!/bin/bash
set -eux

source tools/deployment/store_instantiate.sh "tools/deployment/config_mainet.json" false
source tools/deployment/populate_contracts.sh "tools/deployment/config_mainet.json"