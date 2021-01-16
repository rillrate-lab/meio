if [ -z "$1" ]; then
    echo -e "\nCrate name expected\n"
    exit 1
fi

if [ -z "$2" ]; then
    echo -e "\nVersion expected\n"
    exit 2
fi

while :
do
    echo "Checking crates.io..."
    CURRENT_VERSION=`cargo search "$1" | grep "$1 =" | cut -d '"' -f2`
    if [ "$CURRENT_VERSION" = "$2" ]; then
        break
    else
        echo "Current version is $CURRENT_VERSION waiting for $2"
        sleep 10
    fi
done
