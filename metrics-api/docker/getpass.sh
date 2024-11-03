#!/bin/sh


trap 'stty echo' EXIT

echo -n "Password: "
stty -echo

read pass

stty echo

trap - EXIT

echo -e "\n$pass"
