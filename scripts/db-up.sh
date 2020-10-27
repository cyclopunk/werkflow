#!/bin/bash

docker run -p 9042:9042 --name $1-db --hostname $1-db -d scylladb/scylla
docker run -p 6379:6379 --name $1-cache -d eqalpha/keydb