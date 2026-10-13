
## Mark an Icon

### Mac

```shell
# brew install librsvg
rsvg-convert -w 1024 -h 1024 icon.svg -o icon.png

APP_NAME=rs7z
mkdir -p $APP_NAME.iconset
for size in 16 32 128 256 512; do
    size2=$((size * 2))
    sips -z $size $size icon.png --out $APP_NAME.iconset/icon_${size}x${size}.png
    sips -z $size2 $size2 icon.png --out $APP_NAME.iconset/icon_${size}x${size}@2x.png
done
iconutil -c icns $APP_NAME.iconset --output $APP_NAME.icns
```

### Linux

```shell
mkdir -p icons

for size in 16 22 24 32 48 64 128 256 512; do
    rsvg-convert -w $size -h $size icon.svg -o icons/icon-${size}.png
done

# or png
for size in 16 22 24 32 48 64 128 256 512; do
    sips -z $size $size icon.png --out icons/icon-${size}.png
done
```

