<?php

namespace Psr\Log;

interface LoggerInterface
{
    public function emergency(string $message): void;
}
