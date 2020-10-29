#!/bin/bash

docker run -p 9042:9042 --name $1-db --hostname $1-db -d scylladb/scylla
docker run -p 6379:6379 --name $1-cache -d eqalpha/keydb
docker run -d --hostname $1-mq --name $1-mq -p 8080:15672 -p 5672:5672 rabbitmq:3-management