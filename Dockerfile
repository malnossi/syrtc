FROM alpine:latest
RUN apk --no-cache add ca-certificates tzdata
WORKDIR /app
# Just copy the file you already built on Ubuntu!
COPY ./target/x86_64-unknown-linux-musl/release/aura ./aura-server
COPY index.html .
EXPOSE 8080
CMD ["./aura-server"]