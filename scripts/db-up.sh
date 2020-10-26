#!/bin/bash

docker run -p 9042:9042 --name $1 --hostname $1 -d scylladb/scylla